//! outbound-http-safeguards: native clients never send a request through an environment
//! proxy. Proxy variables are process-wide and reqwest reads them once per process, so this
//! binary sets them, to a recording loopback proxy, before any client is built and holds
//! only tests that expect that environment. Its tests share the proxy's connection count, so
//! each runs alone (`SERIAL`) and compares the count with its own starting value. Loopback
//! only: no DNS and no real network.
use chiral_network::{
    consumer::{Consumer, ConsumerConfig},
    inference_backend::{Backend, Endpoint, PreparedRequest},
    network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    protocol::{self, InferenceOffer, ModelSpec, NetworkIdentity, PriceSchedule},
    provider_config::ModelConfig,
    wallet,
};
use serde_json::{json, Value};
use std::{
    io::Write,
    net::SocketAddr,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};

/// Held by each test for its whole run.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A loopback listener, set as `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` (in both cases,
/// with `NO_PROXY` removed), that counts the connections it accepts and answers none.
fn proxy_connections() -> &'static AtomicUsize {
    static PROXY: OnceLock<&'static AtomicUsize> = OnceLock::new();
    PROXY.get_or_init(|| {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = format!("http://{}", listener.local_addr().unwrap());
        let connections: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                connections.fetch_add(1, Ordering::SeqCst);
                drop(stream);
            }
        });
        for name in ["HTTP_PROXY", "http_proxy", "HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"] {
            std::env::set_var(name, &proxy);
        }
        for name in ["NO_PROXY", "no_proxy"] {
            std::env::remove_var(name);
        }
        connections
    })
}

/// The proxy's connection count after a control request: a client that honours the
/// environment sends a request for `origin` to the proxy, which `origin` never receives.
async fn proxy_in_effect(origin: &str, seen: &Mutex<Vec<String>>) -> (&'static AtomicUsize, usize) {
    let proxy = proxy_connections();
    let before = proxy.load(Ordering::SeqCst);
    assert!(reqwest::Client::new().get(format!("{origin}/healthz")).send().await.is_err());
    assert_eq!(proxy.load(Ordering::SeqCst), before + 1, "the proxy environment is not in effect");
    assert!(seen.lock().unwrap().is_empty());
    (proxy, before + 1)
}

/// A loopback fixture standing in for a provider, a catalog, a private engine and an
/// administration listener. It records each request's method, path and `Authorization`
/// header, and answers `/healthz` live, `/readyz` ready, `/api/offers` with an empty last
/// page, `/health` ok, and everything else with `{"ok":true}`.
async fn recording_fixture() -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address: SocketAddr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let app = axum::Router::new().fallback(
        move |method: axum::http::Method, uri: axum::http::Uri, headers: axum::http::HeaderMap| {
            let recorded = recorded.clone();
            async move {
                let authorization = headers
                    .get(axum::http::header::AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("")
                    .to_string();
                recorded.lock().unwrap().push(format!("{method} {} {authorization}", uri.path()));
                axum::Json(match uri.path() {
                    "/healthz" => json!({"status": "live"}),
                    "/readyz" => json!({"ready": true}),
                    "/api/offers" => json!({"offers": [], "next_offset": null}),
                    "/health" => json!({"status": "ok"}),
                    _ => json!({"ok": true}),
                })
            }
        },
    );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{address}"), seen, task)
}

fn consumer(directory: &Path, catalog_url: Option<String>, development: bool) -> Consumer {
    Consumer::new(ConsumerConfig {
        network: NetworkConfig {
            identity: NetworkIdentity {
                chain_id: ESPACE_TESTNET_CHAIN_ID,
                genesis_hash: ESPACE_TESTNET_GENESIS.into(),
                deployment_id: "proxy-environment-test".into(),
            },
            rpc_url: "https://rpc.example".into(),
            rpc_bearer_file: None,
            faucet_url: None,
            explorer_url: None,
            policy_url: None,
        },
        data_dir: directory.join("data"),
        catalog_url,
        development,
        dht: None,
    })
    .unwrap()
}

fn model() -> ModelSpec {
    ModelSpec {
        id: "model-a".into(),
        revision: "ab".repeat(20),
        tokenizer_revision: "cd".repeat(20),
        chat_template_sha256: "ef".repeat(32),
        quantization: None,
        context_length: 4096,
        max_output_tokens: 128,
        capabilities: vec!["chat".into()],
        serving: None,
    }
}

/// outbound-http-safeguards requirement 6, task 3.6: the native administration client sends
/// the administration token directly to the loopback listener, never through an
/// environment proxy (book line 658: administration requests are local-only).
#[tokio::test]
async fn the_administration_client_never_uses_an_environment_proxy() {
    let _serial = SERIAL.lock().await;
    let (origin, seen, task) = recording_fixture().await;
    let (proxy, count) = proxy_in_effect(&origin, &seen).await;

    let directory = tempfile::tempdir().unwrap();
    let consumer = consumer(directory.path(), None, false);
    let token = "administration-test-token-0123456789abcdef";
    let token_file = directory.path().join("admin.token");
    wallet::create_secret_file(&token_file).unwrap().write_all(token.as_bytes()).unwrap();
    for (action, request) in [("status", "GET /admin/status"), ("drain", "POST /admin/drain")] {
        let answer: Value = consumer.provider_admin(&origin, &token_file, action).await.unwrap();
        assert_eq!(answer, json!({"ok": true}), "{action}");
        assert_eq!(seen.lock().unwrap().pop(), Some(format!("{request} Bearer {token}")));
    }
    assert_eq!(proxy.load(Ordering::SeqCst), count, "the administration token reached the proxy");
    task.abort();
}

/// outbound-http-safeguards requirement 4 scenario "Environment proxies are ignored" (task
/// 3.4). With every proxy variable naming a listening proxy, a development consumer reaches
/// a loopback provider, which is also its catalog, directly through the pinned client of
/// that origin: inspection (`/healthz`, `/readyz` and the reputation read), discovery
/// (`/api/offers`) and quoting (`/v1/contracts/propose`) each arrive at the fixture, and the
/// proxy accepts no connection.
#[tokio::test]
async fn provider_and_catalog_requests_never_use_an_environment_proxy() {
    let _serial = SERIAL.lock().await;
    let (origin, seen, task) = recording_fixture().await;
    let (proxy, count) = proxy_in_effect(&origin, &seen).await;

    let directory = tempfile::tempdir().unwrap();
    let consumer = consumer(directory.path(), Some(origin.clone()), true);
    consumer.wallet_create().unwrap();
    let key = wallet::generate_private_key();
    let provider_wallet = wallet::address_from_private_key(&key).unwrap();
    let now = protocol::now().unwrap();
    let offer = protocol::sign(
        protocol::OFFER_DOMAIN,
        InferenceOffer {
            network: consumer.network_config().identity.clone(),
            provider_wallet: provider_wallet.clone(),
            peer_id: libp2p::identity::Keypair::generate_ed25519().public().to_peer_id().to_string(),
            deployment_id: "proxy-test".into(),
            endpoint: origin.clone(),
            model: model(),
            price: PriceSchedule { input_wei_per_1k: "1".into(), output_wei_per_1k: "2".into() },
            min_funding_wei: "10".into(),
            platform_wallet: format!("0x{}", "33".repeat(20)),
            fee_bps: 50,
            max_concurrency: 1,
            revision: 1,
            issued_at: now - 120,
            expires_at: now + 600,
        },
        &key,
    )
    .unwrap();
    let info = consumer.provider_info(&offer).await.unwrap();
    assert_eq!(info["health"]["live"], true, "{info}");
    assert_eq!(info["readiness"]["ready"], true, "{info}");
    assert!(consumer.discover(None).await.unwrap().is_empty());
    // The fixture's answer is not a signed quote; the request itself went to the provider.
    assert!(consumer.quote(&offer, "100").await.is_err());
    let mut requests = seen.lock().unwrap().clone();
    requests.sort();
    let mut expected = vec![
        "GET /healthz ".to_string(),
        "GET /readyz ".into(),
        format!("GET /api/ratings/{provider_wallet} "),
        "GET /api/offers ".into(),
        "POST /v1/contracts/propose ".into(),
    ];
    expected.sort();
    assert_eq!(requests, expected);
    assert_eq!(proxy.load(Ordering::SeqCst), count, "a provider or catalog request reached the proxy");
    task.abort();
}

/// outbound-http-safeguards requirement 7, task 3.7: the provider's private engine client
/// ignores environment proxies (book line 275). With every proxy variable naming a listening
/// proxy, a `Backend` for a loopback engine sends its readiness probe and an inference
/// dispatch, each with the engine key, directly to the engine, and the proxy accepts no
/// connection.
#[tokio::test]
async fn the_engine_client_never_uses_an_environment_proxy() {
    let _serial = SERIAL.lock().await;
    let (origin, seen, task) = recording_fixture().await;
    let (proxy, count) = proxy_in_effect(&origin, &seen).await;

    let directory = tempfile::tempdir().unwrap();
    let key = "engine-test-key-0123456789abcdef0123456789";
    let key_file = directory.path().join("engine.token");
    wallet::create_secret_file(&key_file).unwrap().write_all(key.as_bytes()).unwrap();
    let backend = Backend::new(&ModelConfig {
        model: model(),
        price: PriceSchedule { input_wei_per_1k: "1".into(), output_wei_per_1k: "2".into() },
        engine_url: origin.clone(),
        engine_api_key_file: Some(key_file),
        runtime: None,
        max_concurrency: 1,
        request_timeout_secs: 30,
        qualification_evidence_file: None,
    })
    .unwrap();
    backend.ready().await.unwrap();
    let request = PreparedRequest {
        endpoint: Endpoint::Chat,
        body: json!({"model": "model-a", "messages": [{"role": "user", "content": "hi"}], "max_tokens": 1}),
        stream: false,
        include_usage: false,
        max_output_tokens: 1,
        prompt_tokens: 1,
        embedding_count: 0,
        logprobs: None,
        echo_prompt: None,
    };
    backend.dispatch(&request, "request-a").await.unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        [format!("GET /health Bearer {key}"), format!("POST /v1/chat/completions Bearer {key}")]
    );
    assert_eq!(proxy.load(Ordering::SeqCst), count, "an engine request reached the proxy");
    task.abort();
}
