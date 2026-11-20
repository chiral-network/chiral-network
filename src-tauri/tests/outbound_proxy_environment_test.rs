//! outbound-http-safeguards: native clients never send a request through an environment
//! proxy. Proxy variables are process-wide and reqwest reads them once per process, so this
//! binary sets them, to a recording loopback proxy, before any client is built and holds
//! only tests that expect that environment. Loopback only: no DNS and no real network.
use chiral_network::{
    consumer::{Consumer, ConsumerConfig},
    network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    protocol::NetworkIdentity,
    wallet,
};
use serde_json::{json, Value};
use std::{
    io::Write,
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
};

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

/// A loopback administration fixture that answers every request with `{"ok":true}` and
/// records its method, path and `Authorization` header.
async fn administration_fixture() -> (String, Arc<Mutex<Vec<String>>>, tokio::task::JoinHandle<()>) {
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
                axum::Json(json!({"ok": true}))
            }
        },
    );
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{address}"), seen, task)
}

/// outbound-http-safeguards requirement 6, task 3.6: the native administration client sends
/// the administration token directly to the loopback listener, never through an
/// environment proxy (book line 658: administration requests are local-only).
#[tokio::test]
async fn the_administration_client_never_uses_an_environment_proxy() {
    let proxy = proxy_connections();
    let (origin, seen, task) = administration_fixture().await;
    // Control: a client that honours the environment sends this very request to the proxy.
    assert!(reqwest::Client::new().get(format!("{origin}/admin/status")).send().await.is_err());
    assert_eq!(proxy.load(Ordering::SeqCst), 1, "the proxy environment is not in effect");
    assert!(seen.lock().unwrap().is_empty());

    let directory = tempfile::tempdir().unwrap();
    let consumer = Consumer::new(ConsumerConfig {
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
        data_dir: directory.path().join("data"),
        catalog_url: None,
        development: false,
        dht: None,
    })
    .unwrap();
    let token = "administration-test-token-0123456789abcdef";
    let token_file = directory.path().join("admin.token");
    wallet::create_secret_file(&token_file).unwrap().write_all(token.as_bytes()).unwrap();
    for (action, request) in [("status", "GET /admin/status"), ("drain", "POST /admin/drain")] {
        let answer: Value = consumer.provider_admin(&origin, &token_file, action).await.unwrap();
        assert_eq!(answer, json!({"ok": true}), "{action}");
        assert_eq!(seen.lock().unwrap().pop(), Some(format!("{request} Bearer {token}")));
    }
    assert_eq!(proxy.load(Ordering::SeqCst), 1, "the administration token reached the proxy");
    task.abort();
}
