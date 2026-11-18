//! The standalone chain RPC gateway has its own protected inbound bearer on every bind address
//! (book lines 474, 644 and 656), refuses browser origins, and exposes an exact `eth_*`
//! allowlist (book line 667). See `openspec/changes/specify-chain-rpc-gateway`.
//!
//! The `chiral_rpc_gateway` binary runs against a loopback upstream fixture that answers only the
//! identity check and `eth_blockNumber`. No live chain is contacted.
use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use chiral_network::network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS};
use chiral_network::protocol::NetworkIdentity;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const INBOUND: &str = "gateway-inbound-bearer-0123456789"; // 33 visible ASCII characters
const UPSTREAM: &str = "upstream-node-bearer-abcdefghijklmnop";
const WAIT: Duration = Duration::from_secs(30);

/// Authorization headers of every request the upstream fixture received, in order.
type Seen = Arc<Mutex<Vec<Option<String>>>>;

async fn upstream_rpc(
    State(seen): State<Seen>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Json<Value> {
    seen.lock().unwrap().push(
        headers
            .get("authorization")
            .map(|value| value.to_str().unwrap().to_owned()),
    );
    let id = request["id"].clone();
    let result = match request["method"].as_str().unwrap_or("") {
        "eth_chainId" => json!(format!("0x{ESPACE_TESTNET_CHAIN_ID:x}")),
        "eth_getBlockByNumber" if request["params"] == json!(["0x0", false]) => json!({
            "number": "0x0", "hash": ESPACE_TESTNET_GENESIS, "timestamp": "0x0", "transactions": []
        }),
        "eth_blockNumber" => json!("0x10"),
        _ => {
            return Json(
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"not served by fixture"}}),
            )
        }
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

/// A loopback upstream that passes the gateway's identity check.
async fn upstream() -> (String, Seen) {
    let seen = Seen::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/", post(upstream_rpc))
        .with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen)
}

fn protected(path: &Path, contents: &str) {
    let mut file = chiral_network::wallet::create_secret_file(path).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
}

/// A gateway configuration in `root`, optionally naming an upstream bearer file next to it.
fn gateway_config(root: &Path, rpc_url: &str, upstream_bearer: Option<&str>) -> PathBuf {
    let rpc_bearer_file = upstream_bearer.map(|token| {
        protected(&root.join("upstream.token"), token);
        PathBuf::from("upstream.token")
    });
    let config = NetworkConfig {
        identity: NetworkIdentity {
            chain_id: ESPACE_TESTNET_CHAIN_ID,
            genesis_hash: ESPACE_TESTNET_GENESIS.into(),
            deployment_id: "rpc-gateway-test".into(),
        },
        rpc_url: rpc_url.into(),
        rpc_bearer_file,
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    };
    let path = root.join("rpc-gateway.json");
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    path
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Kills the gateway if a test ends while it is still running.
struct Gateway(Child);
impl Drop for Gateway {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn(config: &Path, root: &Path, port: u16, bearer: Option<&Path>) -> Gateway {
    let mut command = Command::new(env!("CARGO_BIN_EXE_chiral_rpc_gateway"));
    command
        .arg("--config")
        .arg(config)
        .arg("--bind")
        .arg(format!("127.0.0.1:{port}"))
        .arg("--journal-dir")
        .arg(root.join("journal"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(bearer) = bearer {
        command.arg("--bearer-file").arg(bearer);
    }
    Gateway(command.spawn().unwrap())
}

/// Waits for the gateway to exit and returns its stderr; panics if it keeps running.
async fn refused(mut gateway: Gateway, case: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(status) = gateway.0.try_wait().unwrap() {
            let mut stderr = String::new();
            std::io::Read::read_to_string(gateway.0.stderr.as_mut().unwrap(), &mut stderr)
                .unwrap();
            assert!(!status.success(), "{case}: gateway exited successfully");
            return stderr;
        }
        assert!(Instant::now() < deadline, "{case}: gateway kept running");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// Waits until the running gateway answers `/healthz`.
async fn started(gateway: &mut Gateway, base: &str) {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(status) = gateway.0.try_wait().unwrap() {
            let mut stderr = String::new();
            std::io::Read::read_to_string(gateway.0.stderr.as_mut().unwrap(), &mut stderr)
                .unwrap();
            panic!("gateway exited with {status}: {stderr}");
        }
        if let Ok(response) = http().get(format!("{base}/healthz")).send().await {
            if response.status() == reqwest::StatusCode::OK {
                return;
            }
        }
        assert!(Instant::now() < deadline, "gateway did not start");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_loopback_gateway_without_a_bearer_file_refuses_to_start() {
    let (url, seen) = upstream().await;
    let root = tempfile::tempdir().unwrap();
    let config = gateway_config(root.path(), &url, None);
    let gateway = spawn(&config, root.path(), free_port(), None);
    let stderr = refused(gateway, "loopback without --bearer-file").await;
    assert!(stderr.contains("--bearer-file"), "{stderr}");
    assert!(seen.lock().unwrap().is_empty(), "the chain was contacted");
}

/// How a refused case presents its inbound bearer file.
enum Bearer {
    Protected(String),
    Missing,
    /// Protected inbound bearer equal to the configured upstream bearer.
    Upstream(String),
    #[cfg(unix)]
    Exposed(String),
    #[cfg(unix)]
    Link(String),
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_inbound_bearer_files_refuse_to_start_before_contacting_the_chain() {
    let (url, seen) = upstream().await;
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut cases = vec![
        ("31 characters", Bearer::Protected("a".repeat(31)), "at least 32 visible ASCII characters"),
        ("32 non-ASCII characters", Bearer::Protected("\u{e9}".repeat(32)), "at least 32 visible ASCII characters"),
        ("empty file", Bearer::Protected(String::new()), "secret"),
        ("internal whitespace", Bearer::Protected(format!("{INBOUND} {INBOUND}")), "invalid token"),
        ("missing file", Bearer::Missing, "secret"),
        ("equal to the upstream bearer", Bearer::Upstream(format!("{UPSTREAM}\n")), "must differ from the upstream RPC bearer"),
    ];
    #[cfg(unix)]
    cases.extend([
        ("group- and world-readable file", Bearer::Exposed(INBOUND.into()), "no group/other permissions"),
        ("symbolic link to a protected file", Bearer::Link(INBOUND.into()), "secret"),
    ]);
    for (case, bearer, message) in cases {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        let path = root.join("rpc.token");
        let mut upstream_bearer = None;
        match bearer {
            Bearer::Protected(token) => protected(&path, &token),
            Bearer::Missing => {}
            Bearer::Upstream(token) => {
                protected(&path, &token);
                upstream_bearer = Some(UPSTREAM);
            }
            #[cfg(unix)]
            Bearer::Exposed(token) => {
                use std::os::unix::fs::PermissionsExt;
                protected(&path, &token);
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            }
            #[cfg(unix)]
            Bearer::Link(token) => {
                protected(&root.join("target.token"), &token);
                std::os::unix::fs::symlink(root.join("target.token"), &path).unwrap();
            }
        }
        let config = gateway_config(root, &url, upstream_bearer);
        let gateway = spawn(&config, root, free_port(), Some(&path));
        let stderr = refused(gateway, case).await;
        assert!(stderr.contains(message), "{case}: {stderr}");
        assert!(!stderr.contains(INBOUND) && !stderr.contains(UPSTREAM), "{case}: bearer echoed");
        assert!(seen.lock().unwrap().is_empty(), "{case}: the chain was contacted");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_gateway_serves_rpc_only_with_its_bearer_and_never_to_a_browser_origin() {
    let (url, seen) = upstream().await;
    let root = tempfile::tempdir().unwrap();
    let config = gateway_config(root.path(), &url, Some(UPSTREAM));
    // Exactly 32 characters is the inclusive minimum.
    let token = &INBOUND[..32];
    let bearer = root.path().join("rpc.token");
    protected(&bearer, &format!("{token}\n"));
    let port = free_port();
    let base = format!("http://127.0.0.1:{port}");
    let mut gateway = spawn(&config, root.path(), port, Some(&bearer));
    started(&mut gateway, &base).await;

    // Liveness and readiness are unauthenticated loopback probes.
    let ready = http().get(format!("{base}/readyz")).send().await.unwrap();
    assert_eq!(ready.status(), reqwest::StatusCode::OK);
    assert_eq!(ready.json::<Value>().await.unwrap(), json!({"ready": true}));

    let rpc = format!("{base}/api/chain/rpc");
    let body = json!({"jsonrpc":"2.0","id":7,"method":"eth_blockNumber","params":[]});
    let forwarded = seen.lock().unwrap().len();
    let wrong = format!("{}x", &token[..31]);
    let valid = format!("Bearer {token}");
    for (case, authorization, header) in [
        ("no bearer", None, None),
        ("wrong bearer of equal length", Some(format!("Bearer {wrong}")), None),
        ("prefix of the bearer", Some(format!("Bearer {}", &token[..16])), None),
        ("bearer followed by more", Some(format!("Bearer {token}x")), None),
        ("upstream bearer", Some(format!("Bearer {UPSTREAM}")), None),
        ("bearer without scheme", Some(token.to_owned()), None),
        ("forwarded from loopback", None, Some(("X-Forwarded-For", "127.0.0.1"))),
        ("valid bearer from a browser", Some(valid.clone()), Some(("Origin", "https://example.org"))),
        ("valid bearer with a null origin", Some(valid.clone()), Some(("Origin", "null"))),
    ] {
        let mut request = http().post(&rpc).json(&body);
        if let Some(authorization) = authorization {
            request = request.header("Authorization", authorization);
        }
        if let Some((name, value)) = header {
            request = request.header(name, value);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED, "{case}");
        assert_eq!(response.headers()["cache-control"], "no-store", "{case}");
        let error = response.json::<Value>().await.unwrap();
        assert_eq!(error["error"]["code"], -32003, "{case}");
    }
    // Refusals consume no admission budget: more than the read burst is refused first.
    for _ in 0..200 {
        let response = http().post(&rpc).json(&body).send().await.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    }
    assert_eq!(seen.lock().unwrap().len(), forwarded, "a refused request reached the chain");

    let response = http()
        .post(&rpc)
        .header("Authorization", &valid)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"jsonrpc":"2.0","id":7,"result":"0x10"})
    );

    // Authenticated callers still reach only the eth_* allowlist.
    let response = http()
        .post(&rpc)
        .header("Authorization", &valid)
        .json(&json!({"jsonrpc":"2.0","id":8,"method":"net_version","params":[]}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        response.json::<Value>().await.unwrap()["error"]["message"],
        "RPC method is not exposed"
    );

    // The upstream saw only the upstream credential, never the inbound bearer.
    let seen = seen.lock().unwrap();
    assert!(seen.len() > forwarded);
    assert!(seen
        .iter()
        .all(|authorization| authorization.as_deref() == Some(&*format!("Bearer {UPSTREAM}"))));
}
