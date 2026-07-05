//! Container API — the HTTP surface for the compute data plane. It authenticates
//! a contract's bearer, admits a container against the contract's resource
//! envelope, starts it through the [`ContainerRuntime`] driver, and on stop
//! charges the accrued runtime to the contract balance. Continuous runtime
//! metering is done by a background task calling `ContainerProvider::meter_all`
//! (not shown here); this router covers submit + lifecycle.
//!
//! Design: `docs/chiral-book.md` → "Data-Plane API: Compute (Containers)" and
//! "Provider Implementation" → Container. The real `ContainerRuntime` drives
//! Docker/Podman via `bollard`; a mock drives the tests here (no Docker host).

use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::Deserialize;

use crate::container_provider::{
    ContainerError, ContainerProvider, ContainerRuntime, ResourceEnvelope, ResourceRequest,
};
use crate::contract_service::{ChainVerifier, ProviderState};

pub struct ContainerState<V: ChainVerifier, R: ContainerRuntime> {
    pub provider: ProviderState<V>,
    pub containers: ContainerProvider,
    pub runtime: R,
    /// The resource envelope contracts of this provider may consume (from the
    /// offer's advertised shape).
    pub envelope: ResourceEnvelope,
}

pub type Shared<V, R> = Arc<Mutex<ContainerState<V, R>>>;

#[derive(Debug, Deserialize)]
pub struct ContainerSpec {
    pub image: String,
    pub resources: ResourceRequest,
}

pub fn router<V, R>(state: Shared<V, R>) -> Router
where
    V: ChainVerifier + Send + Sync + 'static,
    R: ContainerRuntime + Send + Sync + 'static,
{
    Router::new()
        .route("/v1/containers", post(create::<V, R>))
        .route("/v1/containers/:id", axum::routing::delete(delete_container::<V, R>))
        .with_state(state)
}

async fn create<V, R>(
    State(state): State<Shared<V, R>>,
    headers: HeaderMap,
    Json(spec): Json<ContainerSpec>,
) -> Response
where
    V: ChainVerifier + Send + Sync + 'static,
    R: ContainerRuntime + Send + Sync + 'static,
{
    let now = now_unix();
    let mut st = state.lock().unwrap();
    let contract = match auth(&st.provider, &headers, now) {
        Ok(c) => c,
        Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let id = format!("ctr_{}", uuid::Uuid::new_v4().simple());
    let envelope = st.envelope;
    if let Err(e) = st.containers.admit(&contract, &id, spec.resources, envelope, now) {
        let code = match e {
            ContainerError::EnvelopeExceeded => "envelope_exceeded",
            ContainerError::NotFound => "not_found",
        };
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": { "code": code } })),
        )
            .into_response();
    }
    match st.runtime.start(&id, &spec.image, spec.resources) {
        Ok(endpoint) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "container_id": id,
                "status": "running",
                "endpoints": [ { "url": endpoint } ]
            })),
        )
            .into_response(),
        Err(e) => {
            // Roll back the admission if the runtime couldn't start it.
            let _ = st.containers.stop(&id, now);
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({ "error": { "code": "start_failed", "message": e } })),
            )
                .into_response()
        }
    }
}

async fn delete_container<V, R>(
    State(state): State<Shared<V, R>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response
where
    V: ChainVerifier + Send + Sync + 'static,
    R: ContainerRuntime + Send + Sync + 'static,
{
    let now = now_unix();
    let mut st = state.lock().unwrap();
    if auth(&st.provider, &headers, now).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match st.containers.stop(&id, now) {
        Ok((contract, cost)) => {
            let _ = st.provider.draw_down(&contract, cost);
            let _ = st.runtime.stop(&id);
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
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

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container_provider::ContainerRates;
    use crate::contract_service::VerifiedFunding;
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
    struct MockRt;
    impl ContainerRuntime for MockRt {
        fn start(&self, id: &str, _image: &str, _req: ResourceRequest) -> Result<String, String> {
            Ok(format!("https://{}.host", id))
        }
        fn stop(&self, _id: &str) -> Result<(), String> {
            Ok(())
        }
    }
    const CONTRACT: &str = "0xfeed00000000000000000000000000000000000000000000000000000000beef";
    const ONE_CHI: u128 = 1_000_000_000_000_000_000;

    fn offer() -> ResourceOffer {
        ResourceOffer {
            provider_wallet: "0xp".into(),
            resource_class: ResourceClass::Container,
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

    fn app() -> (Router, String) {
        let mut provider =
            ProviderState::new("0xp".into(), "0x".to_string() + &"33".repeat(32), offer(), NoChain);
        provider
            .ledger
            .open(CONTRACT, "0xconsumer", ResourceClass::Container, ONE_CHI)
            .unwrap();
        let cred = SessionCredential::derive(CONTRACT, u64::MAX, &[4u8; 64]).unwrap();
        let bearer = cred.bearer.clone();
        provider.sessions.insert(cred);
        let containers = ContainerProvider::new(ContainerRates {
            per_vcpu_hour_wei: 3_600_000,
            per_gb_mem_hour_wei: 360_000,
            per_gpu_hour_wei: 36_000_000,
        });
        let st = ContainerState {
            provider,
            containers,
            runtime: MockRt,
            envelope: ResourceEnvelope { max_vcpu: 4, max_mem_gb: 8, max_gpu: 1 },
        };
        (router(Arc::new(Mutex::new(st))), bearer)
    }

    fn post(uri: &str, bearer: Option<&str>, body: serde_json::Value) -> axum::http::Request<Body> {
        let mut b = axum::http::Request::builder().method("POST").uri(uri).header("content-type", "application/json");
        if let Some(t) = bearer {
            b = b.header(header::AUTHORIZATION, format!("Bearer {}", t));
        }
        b.body(Body::from(body.to_string())).unwrap()
    }

    #[tokio::test]
    async fn create_within_envelope_starts_and_returns_endpoint() {
        let (app, bearer) = app();
        let spec = serde_json::json!({ "image": "nginx", "resources": { "vcpu": 2, "mem_gb": 4 } });
        let r = app.oneshot(post("/v1/containers", Some(&bearer), spec)).await.unwrap();
        assert_eq!(r.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(j["container_id"].as_str().unwrap().starts_with("ctr_"));
        assert!(j["endpoints"][0]["url"].as_str().unwrap().starts_with("https://"));
    }

    #[tokio::test]
    async fn over_envelope_is_409() {
        let (app, bearer) = app();
        let spec = serde_json::json!({ "image": "nginx", "resources": { "vcpu": 5, "mem_gb": 1 } });
        let r = app.oneshot(post("/v1/containers", Some(&bearer), spec)).await.unwrap();
        assert_eq!(r.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn unauthenticated_is_401() {
        let (app, _) = app();
        let spec = serde_json::json!({ "image": "nginx", "resources": { "vcpu": 1, "mem_gb": 1 } });
        let r = app.oneshot(post("/v1/containers", None, spec)).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn delete_unknown_is_404() {
        let (app, bearer) = app();
        let r = app
            .oneshot(
                axum::http::Request::builder()
                    .method("DELETE")
                    .uri("/v1/containers/ctr_nope")
                    .header(header::AUTHORIZATION, format!("Bearer {}", bearer))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }
}
