//! Contract API — the Axum HTTP surface for the provider handshake
//! (`/v1/contracts/*`), a thin wrapper over the [`crate::contract_service`]
//! core. This is what makes a provider actually *serve*: it maps HTTP requests
//! to the core's `propose / open / get / topup / receipt`, supplies the clock
//! and randomness, and maps the core's error codes to HTTP statuses and the
//! spine's JSON error envelope.
//!
//! Mount it into a provider's gateway with [`router`]. The on-chain funding
//! verification is the `ChainVerifier` held in [`ProviderState`] (real impl =
//! `wallet` + `rpc_client`; a mock drives the tests here). The full end-to-end
//! crypto flow is covered by `contract_service`'s tests; these tests assert the
//! HTTP wrapping (routing, JSON bodies, status mapping).

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rand::RngCore;
use serde::Deserialize;

use crate::contract_service::{ChainVerifier, OpenOutcome, ProposeRequest, ProviderState};

pub type Shared<V> = Arc<Mutex<ProviderState<V>>>;

#[derive(Debug, Deserialize)]
pub struct TxRequest {
    pub tx_hash: String,
}

/// Build the `/v1/contracts/*` router over a shared provider state.
pub fn router<V: ChainVerifier + Send + Sync + 'static>(state: Shared<V>) -> Router {
    Router::new()
        .route("/v1/contracts/propose", post(propose::<V>))
        .route("/v1/contracts/open", post(open::<V>))
        .route("/v1/contracts/:id", get(get_contract::<V>))
        .route("/v1/contracts/:id/topup", post(topup::<V>))
        .route("/v1/contracts/:id/receipt", get(receipt::<V>))
        .with_state(state)
}

async fn propose<V: ChainVerifier + Send + Sync + 'static>(
    State(state): State<Shared<V>>,
    Json(req): Json<ProposeRequest>,
) -> Response {
    let now = now_unix();
    let mut nonce = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut nonce);
    let out = { state.lock().unwrap().propose(&req, now, nonce) };
    match out {
        Ok(q) => (StatusCode::OK, Json(q)).into_response(),
        Err(e) => err_response(&e),
    }
}

async fn open<V: ChainVerifier + Send + Sync + 'static>(
    State(state): State<Shared<V>>,
    Json(req): Json<TxRequest>,
) -> Response {
    let now = now_unix();
    let mut entropy = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut entropy);
    let out = { state.lock().unwrap().open(&req.tx_hash, now, &entropy) };
    match out {
        Ok(o @ OpenOutcome::Open { .. }) => (StatusCode::CREATED, Json(o)).into_response(),
        Ok(o @ OpenOutcome::Pending { .. }) => (StatusCode::ACCEPTED, Json(o)).into_response(),
        Err(e) => err_response(&e),
    }
}

async fn get_contract<V: ChainVerifier + Send + Sync + 'static>(
    State(state): State<Shared<V>>,
    Path(id): Path<String>,
) -> Response {
    let view = { state.lock().unwrap().view(&id) };
    match view {
        Some(v) => (StatusCode::OK, Json(v)).into_response(),
        None => err_response("unknown contract"),
    }
}

async fn topup<V: ChainVerifier + Send + Sync + 'static>(
    State(state): State<Shared<V>>,
    Path(_id): Path<String>,
    Json(req): Json<TxRequest>,
) -> Response {
    let out = { state.lock().unwrap().topup(&req.tx_hash) };
    match out {
        Ok(v) => (StatusCode::OK, Json(v)).into_response(),
        Err(e) => err_response(&e),
    }
}

async fn receipt<V: ChainVerifier + Send + Sync + 'static>(
    State(state): State<Shared<V>>,
    Path(id): Path<String>,
) -> Response {
    let now = now_unix();
    let out = { state.lock().unwrap().receipt(&id, now) };
    match out {
        Ok(r) => (StatusCode::OK, Json(r)).into_response(),
        Err(e) => err_response(&e),
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Map a core error string to `(HTTP status, {"error":{code,message}})`.
fn err_response(e: &str) -> Response {
    let status = if e.starts_with("offer_not_found") || e.starts_with("unknown contract") {
        StatusCode::NOT_FOUND
    } else if e.starts_with("below_min_funding") || e.starts_with("capacity") {
        StatusCode::CONFLICT
    } else if e.starts_with("quote_expired") {
        StatusCode::GONE
    } else if e.starts_with("payment_invalid") {
        StatusCode::PAYMENT_REQUIRED
    } else if e.starts_with("tx_not_found") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_REQUEST
    };
    let code = e.split(&[':', ' '][..]).next().unwrap_or("error");
    (
        status,
        Json(serde_json::json!({ "error": { "code": code, "message": e } })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract_service::VerifiedFunding;
    use crate::resource_offer::{ResourceClass, ResourceOffer};
    use axum::body::Body;
    use tower::ServiceExt; // for `oneshot`

    struct NoChain;
    impl ChainVerifier for NoChain {
        fn verify_funding(&self, _tx: &str, _to: &str) -> Result<VerifiedFunding, String> {
            Err("tx_not_found".to_string())
        }
    }

    fn offer() -> ResourceOffer {
        ResourceOffer {
            provider_wallet: "0xprovider0000000000000000000000000000abcd".into(),
            resource_class: ResourceClass::Storage,
            capacity: serde_json::json!({ "gb_available": 100 }),
            price_schedule: serde_json::json!({ "per_gb_month": "10000000000000000" }),
            endpoint: "https://p".into(),
            region: "".into(),
            min_funding_wei: "100000000000000000".into(),
            offer_nonce: 1,
            valid_until: 0,
            signature: String::new(),
        }
    }

    fn app_and_offer_ref() -> (Router, String) {
        let o = offer();
        let offer_ref = o.offer_ref();
        let st = ProviderState::new(
            "0xprovider0000000000000000000000000000abcd".into(),
            "0x3333333333333333333333333333333333333333333333333333333333333333".into(),
            o,
            NoChain,
        );
        (router(Arc::new(Mutex::new(st))), offer_ref)
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    fn post(uri: &str, body: serde_json::Value) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn propose_serves_a_quote() {
        let (app, offer_ref) = app_and_offer_ref();
        let req = serde_json::json!({
            "offer_ref": offer_ref,
            "consumer_wallet": "0xconsumer",
            "funding_amount_wei": "1000000000000000000",
            "params": {}
        });
        let resp = app.oneshot(post("/v1/contracts/propose", req)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let j = body_json(resp).await;
        assert!(j["contract_nonce"].as_str().unwrap().starts_with("0x"));
        assert!(j["terms_hash"].as_str().unwrap().starts_with("0x"));
    }

    #[tokio::test]
    async fn propose_wrong_offer_is_404() {
        let (app, _) = app_and_offer_ref();
        let req = serde_json::json!({
            "offer_ref": "0xdeadbeef",
            "consumer_wallet": "0xc",
            "funding_amount_wei": "1000000000000000000"
        });
        let resp = app.oneshot(post("/v1/contracts/propose", req)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let j = body_json(resp).await;
        assert_eq!(j["error"]["code"], "offer_not_found");
    }

    #[tokio::test]
    async fn below_min_funding_is_409() {
        let (app, offer_ref) = app_and_offer_ref();
        let req = serde_json::json!({
            "offer_ref": offer_ref,
            "consumer_wallet": "0xc",
            "funding_amount_wei": "1"
        });
        let resp = app.oneshot(post("/v1/contracts/propose", req)).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn unknown_contract_get_is_404() {
        let (app, _) = app_and_offer_ref();
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/v1/contracts/0xabc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
