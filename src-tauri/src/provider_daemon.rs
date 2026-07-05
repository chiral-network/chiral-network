//! Provider daemon — turns a [`ProviderConfig`] into a running provider: it
//! builds and signs the offer, constructs the shared `ProviderState` with the
//! real on-chain [`RpcChainVerifier`], assembles the gateway router(s) for the
//! resource class, and serves them with `axum::serve`.
//!
//! Router assembly ([`build`]) is separated from the serve loop ([`run`]) so the
//! config → offer → router path is unit-testable without binding a socket.
//!
//! The compute provider uses the real [`crate::docker_runtime::DockerCliRuntime`]
//! (the `docker` CLI). One marked seam remains: publishing the offer to the DHT
//! so it is discoverable (`// TODO` in [`build`]).

use std::sync::{Arc, Mutex};

use axum::Router;
use serde_json::Value;

use crate::container_provider::{ContainerProvider, ContainerRates, ResourceEnvelope};
use crate::contract_service::{ProviderState, RpcChainVerifier};
use crate::llm_provider::LlmProvider;
use crate::provider_gateway;
use crate::resource_offer::{ResourceClass, ResourceOffer};
use crate::storage_provider::StorageProvider;

/// Everything a provider process needs to run.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub wallet_private_key: String,
    pub region: String,
    /// Public HTTPS base URL advertised in the offer.
    pub endpoint: String,
    /// Control-plane (`/v1/contracts/*`, and the merged data plane) bind address.
    pub control_bind: String,
    /// Storage S3 data-plane bind address (storage serves two listeners).
    pub data_bind: String,
    pub class: ResourceClass,
    pub capacity: Value,
    pub price_schedule: Value,
    pub min_funding_wei: String,
    pub offer_nonce: u64,
    /// Model backend base URL (inference providers).
    pub llm_backend_url: String,
}

impl ProviderConfig {
    /// Load from `CHIRAL_PROVIDER_*` environment variables.
    pub fn from_env() -> Result<Self, String> {
        let get = |k: &str| std::env::var(k).map_err(|_| format!("{k} is required"));
        let opt = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        let class = match opt("CHIRAL_PROVIDER_CLASS", "storage").as_str() {
            "storage" => ResourceClass::Storage,
            "container" => ResourceClass::Container,
            "inference" => ResourceClass::Inference,
            other => return Err(format!("unknown CHIRAL_PROVIDER_CLASS: {other}")),
        };
        let json = |k: &str| -> Result<Value, String> {
            serde_json::from_str(&opt(k, "{}")).map_err(|e| format!("{k}: {e}"))
        };
        Ok(ProviderConfig {
            wallet_private_key: get("CHIRAL_PROVIDER_KEY")?,
            region: opt("CHIRAL_PROVIDER_REGION", ""),
            endpoint: get("CHIRAL_PROVIDER_ENDPOINT")?,
            control_bind: opt("CHIRAL_PROVIDER_BIND", "0.0.0.0:8443"),
            data_bind: opt("CHIRAL_PROVIDER_DATA_BIND", "0.0.0.0:8444"),
            class,
            capacity: json("CHIRAL_PROVIDER_CAPACITY")?,
            price_schedule: json("CHIRAL_PROVIDER_PRICE")?,
            min_funding_wei: opt("CHIRAL_PROVIDER_MIN_FUNDING_WEI", "100000000000000000"),
            offer_nonce: opt("CHIRAL_PROVIDER_OFFER_NONCE", "1").parse().unwrap_or(1),
            llm_backend_url: opt("CHIRAL_PROVIDER_LLM_BACKEND", "http://127.0.0.1:8000"),
        })
    }
}

/// The assembled router(s) for a provider, ready to serve.
pub enum Assembled {
    /// Inference / compute: handshake + data plane merged on one router.
    Single(Router),
    /// Storage: control plane + S3 data plane on separate routers/listeners.
    Storage { control: Router, data: Router },
}

fn address_from_key(pk: &str) -> Result<String, String> {
    let sig = crate::wallet::sign_message(pk, b"chiral-provider-address")?;
    crate::wallet::recover_signer(b"chiral-provider-address", &sig)
}

fn u128_field(v: &Value, key: &str) -> u128 {
    v.get(key).and_then(|x| x.as_str()).and_then(|s| s.parse().ok()).unwrap_or(0)
}
fn u32_field(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(0) as u32
}

/// Build (and sign) the provider's offer, its shared state, and the router(s).
/// Does not bind any socket.
pub fn build(config: &ProviderConfig) -> Result<Assembled, String> {
    let wallet = address_from_key(&config.wallet_private_key)?;
    let mut offer = ResourceOffer {
        provider_wallet: wallet.clone(),
        resource_class: config.class,
        capacity: config.capacity.clone(),
        price_schedule: config.price_schedule.clone(),
        endpoint: config.endpoint.clone(),
        region: config.region.clone(),
        min_funding_wei: config.min_funding_wei.clone(),
        offer_nonce: config.offer_nonce,
        valid_until: 0,
        signature: String::new(),
    };
    offer.sign(&config.wallet_private_key)?;

    let shared = Arc::new(Mutex::new(ProviderState::new(
        wallet,
        config.wallet_private_key.clone(),
        offer,
        RpcChainVerifier,
    )));
    // TODO: publish `shared.lock().offer` to the DHT here so it is discoverable.

    match config.class {
        ResourceClass::Inference => {
            let llm = LlmProvider::from_offer(&shared.lock().unwrap().offer)?;
            Ok(Assembled::Single(provider_gateway::inference_gateway(
                shared,
                llm,
                config.llm_backend_url.clone(),
            )))
        }
        ResourceClass::Container => {
            let rates = ContainerRates {
                per_vcpu_hour_wei: u128_field(&config.price_schedule, "per_vcpu_hour_wei"),
                per_gb_mem_hour_wei: u128_field(&config.price_schedule, "per_gb_mem_hour_wei"),
                per_gpu_hour_wei: u128_field(&config.price_schedule, "per_gpu_hour_wei"),
            };
            let envelope = ResourceEnvelope {
                max_vcpu: u32_field(&config.capacity, "cpu_cores"),
                max_mem_gb: u32_field(&config.capacity, "mem_gb"),
                max_gpu: u32_field(&config.capacity, "gpu_count"),
            };
            Ok(Assembled::Single(provider_gateway::compute_gateway(
                shared,
                ContainerProvider::new(rates),
                crate::docker_runtime::DockerCliRuntime::new(config.endpoint.clone()),
                envelope,
            )))
        }
        ResourceClass::Storage => {
            let store = StorageProvider::new(
                u128_field(&config.price_schedule, "per_gb_egress"),
                u128_field(&config.price_schedule, "per_gb_month"),
                config
                    .capacity
                    .get("max_object_bytes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(5_000_000_000),
            );
            let (control, data) = provider_gateway::storage_control_and_data(shared, store);
            Ok(Assembled::Storage { control, data })
        }
    }
}

/// Build and serve. Runs until the listener(s) close. Requires a multi-threaded
/// Tokio runtime (the `RpcChainVerifier` uses `block_in_place`).
pub async fn run(config: ProviderConfig) -> Result<(), String> {
    match build(&config)? {
        Assembled::Single(router) => serve(&config.control_bind, router).await,
        Assembled::Storage { control, data } => {
            let c = serve(&config.control_bind, control);
            let d = serve(&config.data_bind, data);
            tokio::try_join!(c, d).map(|_| ())
        }
    }
}

async fn serve(addr: &str, router: Router) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    axum::serve(listener, router).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    // A fixed test key -> a derived address; capacity/price shape a valid offer.
    fn inference_config() -> ProviderConfig {
        ProviderConfig {
            wallet_private_key: "0x".to_string() + &"33".repeat(32),
            region: "us-east".into(),
            endpoint: "https://p.example".into(),
            control_bind: "127.0.0.1:0".into(),
            data_bind: "127.0.0.1:0".into(),
            class: ResourceClass::Inference,
            capacity: serde_json::json!({ "models": [ { "id": "llama" } ] }),
            price_schedule: serde_json::json!({ "llama": { "per_1k_input_wei": "1000000", "per_1k_output_wei": "2000000" } }),
            min_funding_wei: "100000000000000000".into(),
            offer_nonce: 1,
            llm_backend_url: "http://127.0.0.1:1".into(),
        }
    }

    #[tokio::test]
    async fn build_assembles_a_serving_inference_router() {
        let router = match build(&inference_config()).unwrap() {
            Assembled::Single(r) => r,
            _ => panic!("inference should assemble a single router"),
        };
        // The assembled router serves the handshake control plane. We recover the
        // signed offer_ref by rebuilding the offer the same way `build` does.
        let cfg = inference_config();
        let wallet = address_from_key(&cfg.wallet_private_key).unwrap();
        let mut offer = ResourceOffer {
            provider_wallet: wallet,
            resource_class: cfg.class,
            capacity: cfg.capacity.clone(),
            price_schedule: cfg.price_schedule.clone(),
            endpoint: cfg.endpoint.clone(),
            region: cfg.region.clone(),
            min_funding_wei: cfg.min_funding_wei.clone(),
            offer_nonce: cfg.offer_nonce,
            valid_until: 0,
            signature: String::new(),
        };
        offer.sign(&cfg.wallet_private_key).unwrap();

        let resp = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/contracts/propose")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({
                            "offer_ref": offer.offer_ref(),
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
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn storage_config_assembles_two_routers() {
        let mut cfg = inference_config();
        cfg.class = ResourceClass::Storage;
        cfg.capacity = serde_json::json!({ "max_object_bytes": 1000000 });
        cfg.price_schedule = serde_json::json!({ "per_gb_egress": "1000", "per_gb_month": "2000" });
        assert!(matches!(build(&cfg).unwrap(), Assembled::Storage { .. }));
    }
}
