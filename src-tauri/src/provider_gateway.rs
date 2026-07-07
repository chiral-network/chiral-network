//! Provider gateway — assembles a runnable provider by mounting the handshake
//! control plane and a data plane over ONE shared `ProviderState`. This is the
//! router a provider binary serves (via `axum::serve` on the provider's public
//! HTTPS listener; see [`crate::contract_service::RpcChainVerifier`] for the
//! on-chain funding check).
//!
//! Storage is special: its S3 object API is a root catch-all (`/:bucket/*key`)
//! that cannot share one router with `/v1/contracts`, so a storage provider
//! serves the control and data planes on two listeners (or the data plane on a
//! bucket subdomain); [`storage_control_and_data`] returns both routers.

use std::sync::{Arc, Mutex};

use axum::Router;

use crate::container_api::{self, ContainerState};
use crate::container_provider::{ContainerProvider, ContainerRuntime, ResourceEnvelope};
use crate::contract_api;
use crate::contract_service::{ChainVerifier, ProviderState};
use crate::llm_api::{self, LlmState};
use crate::llm_provider::LlmProvider;
use crate::storage_api::{self, StorageState};
use crate::storage_provider::StorageProvider;

/// An inference provider: handshake + OpenAI-compatible data plane on one router.
pub fn inference_gateway<V: ChainVerifier + Send + Sync + 'static>(
    shared: Arc<Mutex<ProviderState<V>>>,
    llm: LlmProvider,
    backend_url: String,
) -> Router {
    contract_api::router(shared.clone())
        .merge(llm_api::router(LlmState::new(shared, llm, backend_url)))
}

/// A compute provider: handshake + container data plane on one router.
pub fn compute_gateway<V, R>(
    shared: Arc<Mutex<ProviderState<V>>>,
    containers: ContainerProvider,
    runtime: R,
    envelope: ResourceEnvelope,
) -> Router
where
    V: ChainVerifier + Send + Sync + 'static,
    R: ContainerRuntime + Send + Sync + 'static,
{
    contract_api::router(shared.clone()).merge(container_api::router(ContainerState::new(
        shared, containers, runtime, envelope,
    )))
}

/// A storage provider: the handshake control plane and the S3 data plane as two
/// routers (the S3 root catch-all cannot share a router with `/v1/contracts`).
/// Serve them on two listeners, or the data plane on a bucket subdomain.
pub fn storage_control_and_data<V: ChainVerifier + Send + Sync + 'static>(
    shared: Arc<Mutex<ProviderState<V>>>,
    store: StorageProvider,
) -> (Router, Router) {
    (
        contract_api::router(shared.clone()),
        storage_api::router(StorageState::new(shared, store)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract_service::VerifiedFunding;
    use crate::llm_provider::TokenRates;
    use crate::resource_offer::{ResourceClass, ResourceOffer};
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use tower::ServiceExt;

    struct NoChain;
    impl ChainVerifier for NoChain {
        fn verify_funding(&self, _t: &str, _to: &str) -> Result<VerifiedFunding, String> {
            Err("no".into())
        }
    }

    fn offer() -> ResourceOffer {
        ResourceOffer {
            provider_wallet: "0xp".into(),
            resource_class: ResourceClass::Inference,
            capacity: serde_json::json!({ "models": [ { "id": "llama" } ] }),
            price_schedule: serde_json::json!({}),
            endpoint: "https://p".into(),
            region: "".into(),
            min_funding_wei: "100000000000000000".into(),
            offer_nonce: 1,
            valid_until: 0,
            signature: String::new(),
        }
    }

    /// One assembled router serves BOTH the handshake and the inference data
    /// plane — the shape a provider binary mounts.
    #[tokio::test]
    async fn inference_gateway_serves_both_planes() {
        let offer = offer();
        let offer_ref = offer.offer_ref();
        let shared = Arc::new(Mutex::new(ProviderState::new(
            "0xp".into(),
            "0x".to_string() + &"33".repeat(32),
            offer,
            NoChain,
        )));
        let llm = LlmProvider::new().with_model(
            "llama",
            TokenRates { per_1k_input_wei: 1_000_000, per_1k_output_wei: 2_000_000 },
        );
        let app = inference_gateway(shared, llm, "http://127.0.0.1:1".into());

        // Control plane: propose is reachable and returns a quote.
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/contracts/propose")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "offer_ref": offer_ref,
                            "consumer_wallet": "0xc",
                            "funding_amount_wei": "1000000000000000000",
                            "params": {}
                        })
                        .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "control plane mounted");

        // Data plane: /v1/models is reachable (401 without a session, proving it's
        // mounted and authenticating rather than 404).
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .header(header::AUTHORIZATION, "Bearer chi_sess_none")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "data plane mounted");
    }
}
