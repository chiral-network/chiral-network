//! LLM API — the OpenAI-compatible HTTP surface for the inference data plane.
//! It authenticates a contract's bearer, resolves the requested model, performs
//! worst-case pre-authorization against the contract balance (so a request never
//! exhausts mid-stream), forwards the request to the provider's model backend
//! (llama.cpp / vLLM / Ollama), then meters the backend's returned `usage` and
//! charges the ledger.
//!
//! The state shares `Arc<Mutex<ProviderState>>` with the handshake router (so a
//! session opened via `/v1/contracts/open` authorizes here) plus a read-only
//! `Arc<LlmProvider>` catalog. Design: `docs/chiral-book.md` → "Data-Plane API:
//! Inference (LLM)" and "Provider Implementation" → LLM. The pre-forward guard
//! rails (auth, model-not-found, insufficient balance) are unit-tested; the
//! forward itself is `reqwest` against a live backend.

use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};

use crate::contract_service::{ChainVerifier, ProviderState};
use crate::llm_provider::{estimate_tokens, LlmProvider, Usage};

pub struct LlmState<V: ChainVerifier> {
    pub provider: Arc<Mutex<ProviderState<V>>>,
    pub llm: Arc<LlmProvider>,
    /// Base URL of the local model backend (OpenAI-compatible).
    pub backend_url: String,
}

impl<V: ChainVerifier> LlmState<V> {
    pub fn new(provider: Arc<Mutex<ProviderState<V>>>, llm: LlmProvider, backend_url: String) -> Self {
        LlmState {
            provider,
            llm: Arc::new(llm),
            backend_url,
        }
    }
}

impl<V: ChainVerifier> Clone for LlmState<V> {
    fn clone(&self) -> Self {
        LlmState {
            provider: Arc::clone(&self.provider),
            llm: Arc::clone(&self.llm),
            backend_url: self.backend_url.clone(),
        }
    }
}

pub fn router<V: ChainVerifier + Send + Sync + 'static>(state: LlmState<V>) -> Router {
    Router::new()
        .route("/v1/models", get(models::<V>))
        .route("/v1/chat/completions", post(chat::<V>))
        .with_state(state)
}

struct Proceed {
    contract: String,
    model: String,
    backend_url: String,
}

async fn models<V: ChainVerifier + Send + Sync + 'static>(
    State(st): State<LlmState<V>>,
    headers: HeaderMap,
) -> Response {
    let now = now_unix();
    {
        let p = st.provider.lock().unwrap();
        if auth(&p, &headers, now).is_err() {
            return openai_err(StatusCode::UNAUTHORIZED, "invalid_request_error", "invalid_api_key");
        }
    }
    let data: Vec<serde_json::Value> = st
        .llm
        .model_ids()
        .into_iter()
        .map(|id| serde_json::json!({ "id": id, "object": "model" }))
        .collect();
    Json(serde_json::json!({ "object": "list", "data": data })).into_response()
}

async fn chat<V: ChainVerifier + Send + Sync + 'static>(
    State(st): State<LlmState<V>>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let now = now_unix();
    // Phase 1: auth, model resolution, worst-case pre-authorization.
    let proceed = match prepare_chat(&st, &headers, &body, now) {
        Ok(p) => p,
        Err(resp) => return resp,
    };
    // Phase 2: forward to the model backend (no lock held).
    let (status, json, usage) = match forward(&proceed.backend_url, &body).await {
        Ok(x) => x,
        Err(e) => return openai_err(StatusCode::BAD_GATEWAY, "api_error", &e),
    };
    // Phase 3: meter the actual usage.
    let cost = st
        .llm
        .cost_wei(&proceed.model, usage.prompt_tokens, usage.completion_tokens)
        .unwrap_or(0);
    let balance = st.provider.lock().unwrap().draw_down(&proceed.contract, cost).unwrap_or(0);
    let mut resp = (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), Json(json)).into_response();
    let h = resp.headers_mut();
    h.insert("X-Chiral-Contract-Id", proceed.contract.parse().unwrap());
    h.insert("X-Chiral-Cost-Wei", cost.to_string().parse().unwrap());
    h.insert("X-Chiral-Balance-Wei", balance.to_string().parse().unwrap());
    resp
}

#[allow(clippy::result_large_err)]
fn prepare_chat<V: ChainVerifier>(
    st: &LlmState<V>,
    headers: &HeaderMap,
    body: &serde_json::Value,
    now: u64,
) -> Result<Proceed, Response> {
    let model = body
        .get("model")
        .and_then(|v| v.as_str())
        .ok_or_else(|| openai_err(StatusCode::BAD_REQUEST, "invalid_request_error", "missing model"))?
        .to_string();
    if !st.llm.has_model(&model) {
        return Err(openai_err(StatusCode::NOT_FOUND, "invalid_request_error", "model_not_found"));
    }
    let est = estimate_tokens(&messages_text(body));
    let max_tokens = body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(256);
    let preauth = st.llm.preauth_cost_wei(&model, est, max_tokens).unwrap_or(0);

    let p = st.provider.lock().unwrap();
    let contract = auth(&p, headers, now)
        .map_err(|_| openai_err(StatusCode::UNAUTHORIZED, "invalid_request_error", "invalid_api_key"))?;
    let balance = p.ledger.balance_wei(&contract).unwrap_or(0);
    drop(p);
    if preauth > balance {
        return Err(openai_err(StatusCode::PAYMENT_REQUIRED, "insufficient_quota", "insufficient_balance"));
    }
    Ok(Proceed {
        contract,
        model,
        backend_url: st.backend_url.clone(),
    })
}

async fn forward(
    backend_url: &str,
    body: &serde_json::Value,
) -> Result<(u16, serde_json::Value, Usage), String> {
    let url = format!("{}/v1/chat/completions", backend_url.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .json(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let usage = json
        .get("usage")
        .and_then(|u| serde_json::from_value::<Usage>(u.clone()).ok())
        .unwrap_or(Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
        });
    Ok((status, json, usage))
}

fn auth<V: ChainVerifier>(
    provider: &ProviderState<V>,
    headers: &HeaderMap,
    now: u64,
) -> Result<String, ()> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(())?;
    provider.resolve_bearer(bearer, now).ok_or(())
}

fn messages_text(body: &serde_json::Value) -> String {
    body.get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default()
}

fn openai_err(status: StatusCode, err_type: &str, code: &str) -> Response {
    (
        status,
        Json(serde_json::json!({
            "error": { "message": code, "type": err_type, "param": null, "code": code }
        })),
    )
        .into_response()
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract_service::VerifiedFunding;
    use crate::llm_provider::TokenRates;
    use crate::resource_offer::{ResourceClass, ResourceOffer};
    use crate::session_credential::SessionCredential;
    use axum::body::Body;
    use tower::ServiceExt;

    struct NoChain;
    impl ChainVerifier for NoChain {
        fn verify_funding(&self, _t: &str, _to: &str) -> Result<VerifiedFunding, String> {
            Err("no".into())
        }
    }
    const CONTRACT: &str = "0xfeed00000000000000000000000000000000000000000000000000000000beef";

    fn offer() -> ResourceOffer {
        ResourceOffer {
            provider_wallet: "0xp".into(),
            resource_class: ResourceClass::Inference,
            capacity: serde_json::json!({}),
            price_schedule: serde_json::json!({}),
            endpoint: "https://p".into(),
            region: "".into(),
            min_funding_wei: "0".into(),
            offer_nonce: 1,
            valid_until: 0,
            signature: String::new(),
        }
    }

    fn app(balance_wei: u128, per_1k_out: u128) -> (Router, String) {
        let mut provider = ProviderState::new("0xp".into(), "0x".to_string() + &"33".repeat(32), offer(), NoChain);
        provider.ledger.open(CONTRACT, "0xconsumer", ResourceClass::Inference, balance_wei * 1000 / 995 + 1).unwrap();
        let cred = SessionCredential::derive(CONTRACT, u64::MAX, &[2u8; 64]).unwrap();
        let bearer = cred.bearer.clone();
        provider.sessions.insert(cred);
        let llm = LlmProvider::new().with_model(
            "llama",
            TokenRates { per_1k_input_wei: 1_000_000, per_1k_output_wei: per_1k_out },
        );
        let state = LlmState::new(Arc::new(Mutex::new(provider)), llm, "http://127.0.0.1:1".into());
        (router(state), bearer)
    }

    fn get(uri: &str, bearer: Option<&str>) -> axum::http::Request<Body> {
        let mut b = axum::http::Request::builder().uri(uri);
        if let Some(t) = bearer {
            b = b.header(header::AUTHORIZATION, format!("Bearer {}", t));
        }
        b.body(Body::empty()).unwrap()
    }
    fn post_json(uri: &str, bearer: &str, body: serde_json::Value) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::AUTHORIZATION, format!("Bearer {}", bearer))
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn models_lists_after_auth() {
        let (app, bearer) = app(1_000_000_000_000_000_000, 2_000_000);
        let r = app.clone().oneshot(get("/v1/models", None)).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let r = app.oneshot(get("/v1/models", Some(&bearer))).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(j["data"][0]["id"], "llama");
    }

    #[tokio::test]
    async fn unknown_model_is_404() {
        let (app, bearer) = app(1_000_000_000_000_000_000, 2_000_000);
        let r = app
            .oneshot(post_json("/v1/chat/completions", &bearer, serde_json::json!({ "model": "gpt-4", "messages": [] })))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn insufficient_balance_is_402_before_any_forward() {
        let (app, bearer) = app(1000, 1_000_000_000_000_000);
        let body = serde_json::json!({
            "model": "llama",
            "messages": [ { "role": "user", "content": "hello there general" } ],
            "max_tokens": 100000
        });
        let r = app.oneshot(post_json("/v1/chat/completions", &bearer, body)).await.unwrap();
        assert_eq!(r.status(), StatusCode::PAYMENT_REQUIRED);
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(j["error"]["type"], "insufficient_quota");
    }
}
