//! Provider daemon — turns a [`ProviderConfig`] into a running provider: it
//! builds and signs the offer, constructs the shared `ProviderState` with the
//! real on-chain [`RpcChainVerifier`], assembles the gateway router(s) for the
//! resource class, starts a headless DHT node and **publishes the signed offer**
//! (so it is discoverable), and serves the router(s) with `axum::serve`.
//!
//! Router assembly ([`build`]) is separated from the serve loop ([`run`]) so the
//! config → offer → router path is unit-testable without binding a socket or
//! joining the DHT. Offer publication is best-effort: if the DHT node can't
//! start, the provider still serves (just isn't discoverable yet).
//!
//! The compute provider uses the real [`crate::docker_runtime::DockerCliRuntime`]
//! (the `docker` CLI).

use std::collections::HashMap;
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
/// Returns the signed offer (for publication) and the assembled router(s). Does
/// not bind any socket or touch the DHT.
pub fn build(config: &ProviderConfig) -> Result<(ResourceOffer, Assembled), String> {
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
    let offer_out = offer.clone();

    let shared = Arc::new(Mutex::new(ProviderState::new(
        wallet,
        config.wallet_private_key.clone(),
        offer,
        RpcChainVerifier,
    )));

    let assembled = match config.class {
        ResourceClass::Inference => {
            let llm = LlmProvider::from_offer(&shared.lock().unwrap().offer)?;
            Assembled::Single(provider_gateway::inference_gateway(
                shared,
                llm,
                config.llm_backend_url.clone(),
            ))
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
            Assembled::Single(provider_gateway::compute_gateway(
                shared,
                ContainerProvider::new(rates),
                crate::docker_runtime::DockerCliRuntime::new(config.endpoint.clone()),
                envelope,
            ))
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
            Assembled::Storage { control, data }
        }
    };
    Ok((offer_out, assembled))
}

/// Publish a signed offer to the DHT via a running node.
///
/// Keys the offer record by the node's **peer id** (`peer_dht_key`) so a consumer
/// that enumerated the class index — which resolves to peer ids — can fetch it,
/// and registers the node as a provider for the class index so it shows up in
/// that enumeration.
pub async fn publish_offer(dht: &crate::dht::DhtService, offer: &ResourceOffer) -> Result<String, String> {
    let peer_id = dht.get_peer_id().await.ok_or("DHT has no local peer id yet")?;
    let offer_key = ResourceOffer::peer_dht_key(offer.resource_class, &peer_id);
    let offer_json = serde_json::to_string(offer).map_err(|e| e.to_string())?;
    dht.register_offer(
        offer_key.clone(),
        offer_json,
        ResourceOffer::class_index_key(offer.resource_class),
    )
    .await?;
    Ok(offer_key)
}

/// Start a headless DHT node (a provider needs one only to publish its offer and
/// keep it republished).
async fn start_dht_node() -> Result<Arc<crate::dht::DhtService>, String> {
    use tokio::sync::Mutex as AsyncMutex;
    let ft = Arc::new(AsyncMutex::new(crate::file_transfer::FileTransferService::new()));
    let dd: crate::dht::DownloadDirectoryRef = Arc::new(AsyncMutex::new(None));
    let dc: crate::dht::DownloadCredentialsMap = Arc::new(AsyncMutex::new(HashMap::new()));
    let dht = Arc::new(crate::dht::DhtService::new(ft, dd, dc));
    dht.start_headless().await?;
    dht.wait_for_bootstrap_ready(std::time::Duration::from_secs(60)).await;
    Ok(dht)
}

/// Build, publish the offer to the DHT, and serve. Runs until the listener(s)
/// close. Requires a multi-threaded Tokio runtime (the `RpcChainVerifier` uses
/// `block_in_place`).
pub async fn run(config: ProviderConfig) -> Result<(), String> {
    let (offer, assembled) = build(&config)?;

    // Best-effort discovery: start a DHT node and publish the signed offer.
    let _dht = match start_dht_node().await {
        Ok(dht) => {
            match publish_offer(&dht, &offer).await {
                Ok(key) => println!("[provider] published offer under {key} (offer_ref {})", offer.offer_ref()),
                Err(e) => eprintln!("[provider] offer publish failed: {e}"),
            }
            Some(dht) // keep the node alive while serving
        }
        Err(e) => {
            eprintln!("[provider] DHT node did not start ({e}); serving without discovery");
            None
        }
    };

    match assembled {
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
        // `build` returns the signed offer + the router; the offer_ref it returns
        // is what the served handshake will accept.
        let (offer, assembled) = build(&inference_config()).unwrap();
        let router = match assembled {
            Assembled::Single(r) => r,
            _ => panic!("inference should assemble a single router"),
        };
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
        assert!(matches!(build(&cfg).unwrap().1, Assembled::Storage { .. }));
    }
}
