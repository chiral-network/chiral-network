//! Shared pre-admitted development state; no engine, RPC simulation or qualification claims.
use chiral_network::{
    app::ProviderState,
    billing_store::BillingStore,
    chain_client::ChainClient,
    network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    offer_registry::OfferRegistry,
    protocol::{self, InferenceOffer, ModelSpec, NetworkIdentity, PriceSchedule, Signed, OFFER_DOMAIN},
    provider_config::{DhtConfig, ModelConfig, ProviderConfig},
    wallet,
};
use std::{
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc, RwLock},
};

pub(crate) struct PreparedProvider {
    pub state: Arc<ProviderState>,
    pub offer: Signed<InferenceOffer>,
    pub offer_id: String,
    pub ledger_path: PathBuf,
}

// Restored durable prerequisites, never evidence of observed signing work. Keeping
// both rows in one transaction also makes independent ledger connections realistic.
pub(crate) fn restore_quote_work(path: &Path, minute: u64, count: u64) {
    let mut db = rusqlite::Connection::open(path).unwrap();
    let tx = db.transaction().unwrap();
    tx.execute("INSERT OR REPLACE INTO metadata(key,value) VALUES('quote_work_minute',?1)",
        [minute.to_string()]).unwrap();
    tx.execute("INSERT OR REPLACE INTO rate_limits(bucket,window,count) VALUES('quote-work:global',?1,?2)",
        rusqlite::params![minute, count]).unwrap();
    tx.commit().unwrap();
}

pub(crate) fn quote_work_evidence(path: &Path) -> (String, u64, u64) {
    rusqlite::Connection::open(path).unwrap().query_row(
        "SELECT metadata.value,rate_limits.window,rate_limits.count FROM metadata,rate_limits
         WHERE metadata.key='quote_work_minute' AND rate_limits.bucket='quote-work:global'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap()
}

impl PreparedProvider {
    pub fn new(directory: &Path, case: u64, input: &str, output: &str,
        public_bind: SocketAddr, admin_bind: SocketAddr) -> Self {
        let data_dir = directory.join(format!("provider-{case}"));
        std::fs::create_dir_all(&data_dir).unwrap();
        let base = format!("http://{public_bind}");
        let network = NetworkConfig {
            identity: NetworkIdentity {
                chain_id: ESPACE_TESTNET_CHAIN_ID,
                genesis_hash: ESPACE_TESTNET_GENESIS.into(),
                deployment_id: "proposal-contract-tests".into(),
            },
            // Callers reserve this loopback endpoint; they do not simulate chain/engine replies.
            rpc_url: base.clone(), rpc_bearer_file: None,
            faucet_url: None, explorer_url: None, policy_url: None,
        };
        let key = format!("{:064x}", 2000 + case);
        let provider_wallet = wallet::address_from_private_key(&key).unwrap();
        let signing_key_file = data_dir.join("wallet.key");
        wallet::save_private_key(&signing_key_file, &key).unwrap();
        let admin_token_file = data_dir.join("admin.token");
        wallet::create_secret_file(&admin_token_file).unwrap()
            .write_all(b"public-test-admin-token-not-a-real-secret").unwrap();
        let model = ModelSpec {
            id: "test/proposal".into(), revision: "ab".repeat(20),
            tokenizer_revision: "ab".repeat(20), chat_template_sha256: "cd".repeat(32),
            quantization: Some("Q4_K_M".into()), context_length: 4096, max_output_tokens: 512,
            capabilities: vec!["chat".into(), "completions".into()], serving: None,
        };
        let price = PriceSchedule {
            input_wei_per_1k: input.into(), output_wei_per_1k: output.into(),
        };
        let config = ProviderConfig {
            network: network.clone(), data_dir: data_dir.clone(), signing_key_file, admin_token_file,
            public_endpoint: base.clone(), public_bind, admin_bind,
            platform_wallet: format!("0x{}", "33".repeat(20)), fee_bps: 50,
            min_funding_wei: "100".into(),
            models: vec![ModelConfig {
                model: model.clone(), price: price.clone(), engine_url: base.clone(),
                engine_api_key_file: None, runtime: None, max_concurrency: 2,
                request_timeout_secs: 10, qualification_evidence_file: None,
            }],
            dht: DhtConfig {
                listen: "/ip4/127.0.0.1/tcp/0".into(), bootstrap: vec![],
                identity_file: data_dir.join("dht.key"),
            },
            development: true,
        };
        config.validate().unwrap();
        let now = protocol::now().unwrap();
        let offer = protocol::sign(OFFER_DOMAIN, InferenceOffer {
            network: network.identity.clone(), provider_wallet: provider_wallet.clone(),
            peer_id: "QmPVGjYFugq4XUyBfoTHG6c3qxfBS26jEdaFM1gdAVuMZ2".into(),
            deployment_id: "test-provider".into(), endpoint: base, model, price,
            min_funding_wei: config.min_funding_wei.clone(),
            platform_wallet: config.platform_wallet.clone(), fee_bps: 50,
            max_concurrency: 2, revision: 1, issued_at: now, expires_at: now + 3600,
        }, &key).unwrap();
        let offers = Arc::new(OfferRegistry::new(network.identity.clone(), true));
        let offer_id = offers.insert(offer.clone()).unwrap();
        let ledger_path = data_dir.join("billing.sqlite");
        let ledger = Arc::new(BillingStore::open(&ledger_path, &network.identity).unwrap());
        // An initialized, exhausted previous minute is a restored prerequisite,
        // not 600 signatures performed by this fixture or a production bypass.
        restore_quote_work(&ledger_path, now / 60 - 1, 600);
        let state = Arc::new(ProviderState {
            model_slots: ProviderState::slots(&config), managed_engines: Default::default(),
            chain: Arc::new(ChainClient::new(network, data_dir).unwrap()),
            config: Arc::new(config), ledger, private_key: Arc::new(key), wallet: provider_wallet,
            offers, accepting: AtomicBool::new(true), funding_ready: AtomicBool::new(true),
            // Unprofiled development model admitted as a prerequisite, not measured qualification.
            qualified_models: RwLock::new([offer.payload.model.id.clone()].into_iter().collect()),
            embedding_dimensions: RwLock::new(Default::default()),
        });
        Self { state, offer, offer_id, ledger_path }
    }
}
