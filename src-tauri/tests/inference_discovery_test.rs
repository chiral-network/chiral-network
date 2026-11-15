//! Isolated integration coverage for the production discovery service. No fixed
//! public relay, legacy file protocol, NAT traversal, or opt-in skipped success.
use chiral_network::{
    dht::DhtService,
    network::{ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    offer_registry::OfferRegistry,
    protocol::{self, InferenceOffer, ModelSpec, NetworkIdentity, PriceSchedule},
    provider_config::DhtConfig,
    wallet,
};
use std::{path::Path, sync::Arc};

fn network(deployment: &str) -> NetworkIdentity {
    NetworkIdentity {
        chain_id: ESPACE_TESTNET_CHAIN_ID,
        genesis_hash: ESPACE_TESTNET_GENESIS.into(),
        deployment_id: deployment.into(),
    }
}
fn config(directory: &Path, name: &str, bootstrap: Vec<String>) -> DhtConfig {
    DhtConfig {
        listen: "/ip4/127.0.0.1/tcp/0".into(),
        bootstrap,
        identity_file: directory.join(format!("{name}.key")),
    }
}
#[tokio::test]
async fn wrong_network_bootstrap_cannot_supply_marketplace_offers() {
    let directory = tempfile::tempdir().unwrap();
    let a = network("test-a");
    let b = network("test-b");
    let provider = DhtService::start(
        config(directory.path(), "provider", vec![]),
        a.clone(),
        Arc::new(OfferRegistry::new(a.clone(), true)),
    )
    .await
    .unwrap();
    let key = "1111111111111111111111111111111111111111111111111111111111111111";
    let now = protocol::now().unwrap();
    let offer = protocol::sign(
        protocol::OFFER_DOMAIN,
        InferenceOffer {
            network: a,
            provider_wallet: wallet::address_from_private_key(key).unwrap(),
            peer_id: provider.peer_id(),
            deployment_id: "gpu-a".into(),
            endpoint: "http://127.0.0.1:8080/".into(),
            model: ModelSpec {
                id: "model".into(),
                revision: "ab".repeat(20),
                tokenizer_revision: "ab".repeat(20),
                chat_template_sha256: "ab".repeat(32),
                quantization: None,
                context_length: 4096,
                max_output_tokens: 512,
                capabilities: vec!["chat".into()],
                serving: None,
            },
            price: PriceSchedule {
                input_wei_per_1k: "1".into(),
                output_wei_per_1k: "2".into(),
            },
            min_funding_wei: "1".into(),
            platform_wallet: format!("0x{}", "22".repeat(20)),
            fee_bps: 50,
            max_concurrency: 1,
            revision: 1,
            issued_at: now,
            expires_at: now + 120,
        },
        key,
    )
    .unwrap();
    provider.publish(&offer).await.unwrap();
    let status = provider.status().await;
    let bootstrap = format!(
        "{}/p2p/{}",
        status["listening_addresses"][0].as_str().unwrap(),
        provider.peer_id()
    );
    let registry = Arc::new(OfferRegistry::new(b.clone(), true));
    assert!(registry.insert(offer).is_err());
    let consumer = DhtService::start(
        config(directory.path(), "consumer", vec![bootstrap]),
        b,
        registry,
    )
    .await
    .unwrap();
    assert!(consumer.discover(None).await.unwrap().is_empty());
    consumer.stop().await;
    provider.stop().await;
}
#[tokio::test]
async fn restarted_service_keeps_peer_identity_without_bootstrap() {
    let directory = tempfile::tempdir().unwrap();
    let network = network("restart");
    let config = config(directory.path(), "peer", vec![]);
    let first = DhtService::start(
        config.clone(),
        network.clone(),
        Arc::new(OfferRegistry::new(network.clone(), true)),
    )
    .await
    .unwrap();
    let identity = first.peer_id();
    first.stop().await;
    let second = DhtService::start(
        config,
        network.clone(),
        Arc::new(OfferRegistry::new(network, true)),
    )
    .await
    .unwrap();
    assert_eq!(identity, second.peer_id());
    assert_eq!(second.status().await["published_offers"], 0);
    second.stop().await;
}
