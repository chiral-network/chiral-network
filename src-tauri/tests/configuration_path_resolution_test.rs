//! Relay and RPC-gateway configuration files resolve their relative paths against their own
//! directory, never against the process working directory, while path arguments given on the
//! command line follow the working directory (book line 449; `docs/provider-qualification.md`
//! line 173). See `openspec/changes/specify-profile-path-resolution`, requirement 1 (task 3.1).
//!
//! The real `relay_server` and `chiral_rpc_gateway` binaries run with their configuration file in
//! one temporary directory and a different working directory that holds decoy files under the
//! same relative names. Resolution is observed only through what each process reads and creates:
//! the peer identity the relay reports, the version policy it serves and persists, the bearer a
//! loopback upstream fixture receives, which inbound bearer the gateway accepts, and where state
//! and journals appear. Every wait is bounded, ports are free loopback ports, every child is
//! killed, no live chain is contacted, and no bearer is printed.
use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
use chiral_network::dht::PeerIdentity;
use chiral_network::network::{NetworkConfig, ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS};
use chiral_network::protocol::NetworkIdentity;
use chiral_network::version::{self, VersionPolicy};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(60);
const CONFIGURED_BEARER: &str = "upstream-bearer-next-to-the-configuration-0123";
const DECOY_BEARER: &str = "upstream-bearer-in-the-working-directory-4567";
const CONFIGURED_POLICY: &str = "policy next to the configuration";
const DECOY_POLICY: &str = "policy in the working directory";

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

/// A loopback upstream that passes the chain identity check and records each bearer.
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

/// Whether the fixture was contacted, and only ever with `token`. The tokens are compared, never
/// printed.
fn only_bearer(seen: &Seen, token: &str) -> bool {
    let expected = format!("Bearer {token}");
    let seen = seen.lock().unwrap();
    !seen.is_empty() && seen.iter().all(|value| value.as_deref() == Some(&*expected))
}

/// A temporary root whose paths match what a child reports (macOS temporary directories sit
/// behind a symbolic link).
fn root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().canonicalize().unwrap();
    (directory, path)
}

fn folder(path: PathBuf) -> PathBuf {
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// An owner-only secret file, as operators provision bearers.
fn protected(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = chiral_network::wallet::create_secret_file(path).unwrap();
    file.write_all(contents.as_bytes()).unwrap();
}

fn network(deployment: &str, rpc_url: &str, rpc_bearer_file: PathBuf) -> NetworkConfig {
    NetworkConfig {
        identity: NetworkIdentity {
            chain_id: ESPACE_TESTNET_CHAIN_ID,
            genesis_hash: ESPACE_TESTNET_GENESIS.into(),
            deployment_id: deployment.into(),
        },
        rpc_url: rpc_url.into(),
        rpc_bearer_file: Some(rpc_bearer_file),
        faucet_url: None,
        explorer_url: None,
        policy_url: None,
    }
}

/// Every directory and file under `path`, with the SHA-256 of each file's contents, to prove
/// nothing was touched; a failure shows digests, never a key or bearer.
fn snapshot(path: &Path) -> BTreeMap<PathBuf, Option<String>> {
    let mut entries = BTreeMap::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                entries.insert(path.clone(), None);
                pending.push(path);
            } else {
                let digest = <sha2::Sha256 as sha2::Digest>::digest(std::fs::read(&path).unwrap());
                entries.insert(path.clone(), Some(hex::encode(digest)));
            }
        }
    }
    entries
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// Kills the child when a test ends, whether it passed or failed.
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A child in `current_dir` whose stderr goes to `log`, with no proxy inherited for its loopback
/// requests.
fn command(binary: &str, current_dir: &Path, log: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .current_dir(current_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(log).unwrap());
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
        command.env_remove(name);
    }
    command
}

fn exited(process: &mut Process, log: &Path) -> Option<String> {
    process.0.try_wait().unwrap().map(|status| {
        format!("exited with {status}: {}", std::fs::read_to_string(log).unwrap_or_default())
    })
}

/// A policy signed by `signing`, which permits this client version.
fn signed_policy(signing: &SigningKey, message: &str, issued_at: u64) -> VersionPolicy {
    let mut policy = VersionPolicy {
        min_required: "0.0.0".into(),
        recommended: "0.0.0".into(),
        download_url: "https://example.org/chiral".into(),
        message: Some(message.into()),
        issued_at,
        valid_until: 0,
        signature: String::new(),
    };
    policy.signature =
        hex::encode(signing.sign(&version::canonical_signing_payload(&policy)).to_bytes());
    policy
}

/// Starts `relay_server` and waits for its listening line; returns the reported peer id.
fn start_relay(
    current_dir: &Path,
    config: &Path,
    policy_key: &str,
    log: &Path,
) -> (Process, String) {
    let mut command = command(env!("CARGO_BIN_EXE_relay_server"), current_dir, log);
    command
        .arg("--config")
        .arg(config)
        .env("CHIRAL_POLICY_PUBLIC_KEY", policy_key);
    let mut process = Process(command.spawn().unwrap());
    let stdout = process.0.stdout.take().unwrap();
    let (lines, received) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + WAIT;
    loop {
        match received.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => {
                if let Some(peer) = line.split("peer_id=").nth(1) {
                    assert!(line.starts_with("Chiral testnet relay listening on"), "{line}");
                    return (process, peer.trim().to_owned());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let deadline = Instant::now() + Duration::from_secs(10);
                while exited(&mut process, log).is_none() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(50));
                }
                panic!("relay closed stdout before listening: {:?}", exited(&mut process, log));
            }
        }
        if let Some(exit) = exited(&mut process, log) {
            panic!("relay {exit}");
        }
        assert!(Instant::now() < deadline, "relay did not start");
    }
}

/// profile-path-resolution requirement 1 for the relay configuration (task 3.1): `data_dir`,
/// `dht.identity_file`, `network.rpc_bearer_file` and `version_policy_file` resolve against the
/// configuration file's directory, from an absolute and from a relative `--config`, while the
/// working directory holds a different identity, bearer and signed policy under the same
/// relative names; absolute values are used unchanged from a configuration elsewhere. The
/// configuration's `data_dir` and identity file do not exist before the first start: resolution
/// does not require them, and the relay creates them beside the configuration.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_server_resolves_every_relative_configuration_path_against_its_file() {
    let (url, seen) = upstream().await;
    let (_guard, root) = root();
    let configured = folder(root.join("relay"));
    let work = folder(root.join("work"));
    let other = folder(root.join("other"));
    let logs = folder(root.join("logs"));

    // The same relative names exist in both places (the identity beside the configuration only
    // from the first start on); only the configuration's copies are right.
    let identity = Path::new("keys").join("relay.key");
    let bearer = Path::new("secrets").join("rpc.token");
    let policy_file = Path::new("policy").join("relay-policy.json");
    let decoy_peer = PeerIdentity::load(&work.join(&identity)).unwrap().peer_id();
    // The configuration's identity and data_dir do not exist yet; the first start creates them.
    assert!(!configured.join(&identity).exists() && !configured.join("data").exists());
    let mut configured_identity: Option<(String, Vec<u8>)> = None;
    protected(&configured.join(&bearer), CONFIGURED_BEARER);
    protected(&work.join(&bearer), DECOY_BEARER);
    let signing = SigningKey::from_bytes(&[0x5a; 32]);
    let policy_key = hex::encode(signing.verifying_key().to_bytes());
    let issued_at = chiral_network::protocol::now().unwrap() - 60;
    for (directory, message) in [(&configured, CONFIGURED_POLICY), (&work, DECOY_POLICY)] {
        let path = directory.join(&policy_file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let policy = signed_policy(&signing, message, issued_at);
        std::fs::write(path, serde_json::to_vec(&policy).unwrap()).unwrap();
    }
    let work_before = snapshot(&work);
    let network = network("relay-path-resolution", &url, bearer.clone());
    let state = network.scoped_dir(&configured.join("data"));

    let relay_config = |data_dir: PathBuf, identity: PathBuf, bearer: PathBuf, policy: PathBuf, port: u16| {
        let mut network = network.clone();
        network.rpc_bearer_file = Some(bearer);
        json!({
            "network": network,
            "data_dir": data_dir,
            "bind": format!("127.0.0.1:{port}"),
            "dht": {"listen": "/ip4/127.0.0.1/tcp/0", "bootstrap": [], "identity_file": identity},
            "development": true,
            "version_policy_file": policy,
        })
    };
    let relative_file = configured.join("relay.json");
    let absolute_file = other.join("relay.json");
    for (case, file, argument) in [
        ("absolute --config", &relative_file, relative_file.clone()),
        ("relative --config", &relative_file, Path::new("..").join("relay").join("relay.json")),
        ("absolute paths in a configuration elsewhere", &absolute_file, absolute_file.clone()),
    ] {
        let port = free_port();
        let document = if file == &absolute_file {
            relay_config(
                configured.join("data"),
                configured.join(&identity),
                configured.join(&bearer),
                configured.join(&policy_file),
                port,
            )
        } else {
            relay_config("data".into(), identity.clone(), bearer.clone(), policy_file.clone(), port)
        };
        std::fs::write(file, serde_json::to_vec(&document).unwrap()).unwrap();
        seen.lock().unwrap().clear();
        let log = logs.join(format!("relay-{port}.log"));

        let (mut relay, peer) = start_relay(&work, &argument, &policy_key, &log);

        // dht.identity_file: the identity next to the configuration is created by the first
        // start, then read and never replaced.
        let identity_file = configured.join(&identity);
        assert!(identity_file.is_file(), "{case}: identity file next to the configuration");
        let identity_bytes = std::fs::read(&identity_file).unwrap();
        assert_ne!(peer, decoy_peer, "{case}: identity file");
        assert_eq!(peer, PeerIdentity::load(&identity_file).unwrap().peer_id(), "{case}: identity file");
        match &configured_identity {
            None => configured_identity = Some((peer.clone(), identity_bytes)),
            // Compared, never printed.
            Some((first, bytes)) => {
                assert_eq!(&peer, first, "{case}: identity file");
                assert!(*bytes == identity_bytes, "{case}: identity replaced");
            }
        }
        // version_policy_file: the signed policy next to the configuration is the one in force.
        let served: Value = http()
            .get(format!("http://127.0.0.1:{port}/api/version-policy"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(served["message"], CONFIGURED_POLICY, "{case}: version policy file");
        // network.rpc_bearer_file: the chain identity check carried the configuration's bearer.
        assert!(only_bearer(&seen, CONFIGURED_BEARER), "{case}: RPC bearer file");
        // data_dir: the journal, reputation store and policy watermark are next to the
        // configuration.
        assert!(
            state.join("wallet-journal").join("transactions.sqlite3").is_file(),
            "{case}: data_dir journal"
        );
        assert!(state.join("feedback.sqlite").is_file(), "{case}: data_dir feedback store");
        assert!(
            state.join("version-policy").join(&policy_key).join("policy.json").is_file(),
            "{case}: data_dir policy watermark"
        );
        assert!(exited(&mut relay, &log).is_none(), "{case}: relay stopped");
        drop(relay);

        // Nothing in the working directory was read into use, created or changed.
        assert_eq!(snapshot(&work), work_before, "{case}: working directory changed");
    }
    // Absolute values are used unchanged: nothing was created beside the other configuration.
    let names: Vec<_> = std::fs::read_dir(&other)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(names, ["relay.json"]);
}

/// Starts the gateway from `current_dir` with `--config config`, the relative `--journal-dir
/// journal` and `--bearer-file inbound.token`, and waits until `/healthz` answers.
async fn start_gateway(current_dir: &Path, config: &Path, port: u16, log: &Path) -> Process {
    let mut command = command(env!("CARGO_BIN_EXE_chiral_rpc_gateway"), current_dir, log);
    command
        .arg("--config")
        .arg(config)
        .args(["--journal-dir", "journal", "--bearer-file", "inbound.token"])
        .arg("--bind")
        .arg(format!("127.0.0.1:{port}"));
    let mut process = Process(command.spawn().unwrap());
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(exit) = exited(&mut process, log) {
            panic!("gateway {exit}");
        }
        if let Ok(response) = http().get(format!("http://127.0.0.1:{port}/healthz")).send().await {
            if response.status() == reqwest::StatusCode::OK {
                return process;
            }
        }
        assert!(Instant::now() < deadline, "gateway did not start");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// profile-path-resolution requirement 1 for the RPC-gateway configuration (task 3.1): the
/// configuration's `rpc_bearer_file` resolves against the configuration file's directory, from
/// an absolute and from a relative `--config`, while the command-line `--journal-dir` and
/// `--bearer-file` follow the working directory. Both directories hold an upstream bearer and an
/// inbound bearer under the same relative names.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_rpc_gateway_resolves_its_upstream_bearer_against_its_file_and_its_arguments_against_the_working_directory() {
    const INBOUND: &str = "gateway-inbound-bearer-in-the-working-directory";
    const DECOY_INBOUND: &str = "gateway-inbound-bearer-next-to-the-configuration";
    let (url, seen) = upstream().await;
    let (_guard, root) = root();
    let configured = folder(root.join("gateway"));
    let work = folder(root.join("work"));
    let logs = folder(root.join("logs"));
    let upstream_bearer = Path::new("secrets").join("upstream.token");
    protected(&configured.join(&upstream_bearer), CONFIGURED_BEARER);
    protected(&work.join(&upstream_bearer), DECOY_BEARER);
    protected(&work.join("inbound.token"), INBOUND);
    protected(&configured.join("inbound.token"), DECOY_INBOUND);
    let network = network("gateway-path-resolution", &url, upstream_bearer.clone());
    let config = configured.join("rpc-gateway.json");
    std::fs::write(&config, serde_json::to_vec(&network).unwrap()).unwrap();
    // The same network with the absolute bearer path, in a configuration elsewhere.
    let other = folder(root.join("other"));
    let absolute = other.join("rpc-gateway.json");
    let mut elsewhere = network.clone();
    elsewhere.rpc_bearer_file = Some(configured.join(&upstream_bearer));
    std::fs::write(&absolute, serde_json::to_vec(&elsewhere).unwrap()).unwrap();
    let configured_before = snapshot(&configured);
    let other_before = snapshot(&other);
    let journal = network
        .scoped_dir(&work.join("journal"))
        .join("wallet-journal")
        .join("transactions.sqlite3");

    let relative = Path::new("..").join("gateway").join("rpc-gateway.json");
    for (case, argument) in [
        ("absolute --config", config.as_path()),
        ("relative --config", relative.as_path()),
        ("absolute bearer path in a configuration elsewhere", absolute.as_path()),
    ] {
        seen.lock().unwrap().clear();
        let port = free_port();
        let log = logs.join(format!("gateway-{port}.log"));
        let mut gateway = start_gateway(&work, argument, port, &log).await;

        // rpc_bearer_file: the identity check carried the configuration's upstream bearer.
        assert!(only_bearer(&seen, CONFIGURED_BEARER), "{case}: upstream bearer file");
        // --journal-dir: the journal is in the working directory, not beside the configuration.
        assert!(journal.is_file(), "{case}: --journal-dir");
        // --bearer-file: the inbound bearer is the working directory's file.
        let rpc = format!("http://127.0.0.1:{port}/api/chain/rpc");
        let body = json!({"jsonrpc":"2.0","id":7,"method":"eth_blockNumber","params":[]});
        let accepted = http()
            .post(&rpc)
            .header("Authorization", format!("Bearer {INBOUND}"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(accepted.status(), reqwest::StatusCode::OK, "{case}: --bearer-file");
        assert_eq!(accepted.json::<Value>().await.unwrap()["result"], "0x10");
        let decoy = http()
            .post(&rpc)
            .header("Authorization", format!("Bearer {DECOY_INBOUND}"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(decoy.status(), reqwest::StatusCode::UNAUTHORIZED, "{case}: --bearer-file");
        assert!(only_bearer(&seen, CONFIGURED_BEARER), "{case}: forwarded upstream bearer");
        assert!(exited(&mut gateway, &log).is_none(), "{case}: gateway stopped");
        drop(gateway);

        // Nothing was created or changed beside either configuration.
        assert_eq!(snapshot(&configured), configured_before, "{case}: configuration directory changed");
        assert_eq!(snapshot(&other), other_before, "{case}: other configuration directory changed");
    }
}

/// Runs a child to completion within `WAIT`, its stdout and stderr in `logs`; returns whether it
/// succeeded, its stdout and its stderr.
fn finish(mut command: Command, logs: &Path, name: &str) -> (bool, String, String) {
    let (out, err) = (logs.join(format!("{name}.out")), logs.join(format!("{name}.err")));
    command
        .stdout(std::fs::File::create(&out).unwrap())
        .stderr(std::fs::File::create(&err).unwrap());
    let mut process = Process(command.spawn().unwrap());
    let deadline = Instant::now() + WAIT;
    let status = loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "{name} did not finish");
        std::thread::sleep(Duration::from_millis(20));
    };
    let read = |path: &Path| std::fs::read_to_string(path).unwrap();
    (status.success(), read(&out), read(&err))
}

/// profile-path-resolution requirement 1 for the provider configuration as a process (task 3.1):
/// `chiral_daemon --check`, run from an empty working directory with an absolute and with a
/// relative `--config`, reads its signing key, administration token, engine API key and RPC
/// bearer next to the configuration and restores the policy watermark under the `data_dir` there,
/// starting no service; the working directory stays empty. The command-line `--version-policy`
/// then follows the working directory: a signed policy there (and none beside the configuration)
/// is accepted and persisted under the configuration's `data_dir`. (`ProviderConfig::load`'s
/// library test covers all nine path fields, including those `--check` does not read.)
#[test]
fn the_provider_daemon_reads_its_configured_files_next_to_its_configuration() {
    let (_guard, root) = root();
    let configured = folder(root.join("provider"));
    let work = folder(root.join("work"));
    let logs = folder(root.join("logs"));
    folder(configured.join("keys"));
    let key = chiral_network::wallet::generate_private_key();
    chiral_network::wallet::save_private_key(&configured.join("keys").join("signing.key"), &key).unwrap();
    protected(&configured.join("keys").join("admin.token"), "administration-token-0123456789abcdefghij");
    protected(&configured.join("secrets").join("engine.key"), "engine-api-key-0123456789abcdefghij");
    protected(&configured.join("secrets").join("rpc.token"), CONFIGURED_BEARER);
    let network = network("provider-path-resolution", "http://127.0.0.1:9", Path::new("secrets").join("rpc.token"));
    let document = json!({
        "network": network,
        "data_dir": "data", "signing_key_file": "keys/signing.key", "admin_token_file": "keys/admin.token",
        "public_endpoint": "http://127.0.0.1:18082",
        "public_bind": "127.0.0.1:18082", "admin_bind": "127.0.0.1:18083",
        "platform_wallet": "0x2222222222222222222222222222222222222222",
        "fee_bps": 50, "min_funding_wei": "1000",
        "models": [{
            "model": {
                "id": "test/model", "revision": "ab".repeat(20),
                "tokenizer_revision": "cd".repeat(20), "chat_template_sha256": "ef".repeat(32),
                "quantization": null, "context_length": 4096, "max_output_tokens": 256,
                "capabilities": ["chat"], "serving": null
            },
            "price": { "input_wei_per_1k": "1000", "output_wei_per_1k": "1000" },
            "engine_url": "http://127.0.0.1:18080", "engine_api_key_file": "secrets/engine.key",
            "runtime": null, "max_concurrency": 1, "request_timeout_secs": 30,
            "qualification_evidence_file": null
        }],
        "dht": { "listen": "/ip4/127.0.0.1/tcp/0", "bootstrap": [], "identity_file": "keys/dht.key" },
        "development": true
    });
    let config = configured.join("provider.json");
    std::fs::write(&config, serde_json::to_vec(&document).unwrap()).unwrap();
    let before = snapshot(&configured);
    let watermark = network.scoped_dir(&configured.join("data")).join("version-policy");

    let relative = Path::new("..").join("provider").join("provider.json");
    for (case, argument) in [("absolute --config", config.as_path()), ("relative --config", relative.as_path())] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_chiral_daemon"));
        command
            .current_dir(&work)
            .env_remove("CHIRAL_PROVIDER_CONFIG")
            .stdin(Stdio::null())
            .arg("--check")
            .arg("--config")
            .arg(argument);
        let (success, stdout, stderr) = finish(command, &logs, &case.replace(' ', "-"));
        assert!(success, "{case}: {stderr}");
        assert_eq!(
            stdout.trim(),
            "Provider configuration and protected credentials validated; no services started",
            "{case}"
        );
        // data_dir: the policy watermark directory is next to the configuration.
        assert!(watermark.is_dir(), "{case}: data_dir");
        // Nothing was read from or created in the working directory, and every configured file
        // is unchanged.
        assert!(std::fs::read_dir(&work).unwrap().next().is_none(), "{case}: {:?}", snapshot(&work));
        let after: BTreeMap<_, _> = snapshot(&configured)
            .into_iter()
            .filter(|(path, _)| !path.starts_with(configured.join("data")))
            .collect();
        assert_eq!(after, before, "{case}");
    }

    // --version-policy is a command-line path: it follows the working directory.
    let signing = SigningKey::from_bytes(&[0x6b; 32]);
    let policy_key = hex::encode(signing.verifying_key().to_bytes());
    let issued_at = chiral_network::protocol::now().unwrap() - 60;
    let policy = signed_policy(&signing, "policy in the working directory", issued_at);
    std::fs::write(work.join("policy.json"), serde_json::to_vec(&policy).unwrap()).unwrap();
    let work_before = snapshot(&work);
    let mut command = Command::new(env!("CARGO_BIN_EXE_chiral_daemon"));
    command
        .current_dir(&work)
        .env_remove("CHIRAL_PROVIDER_CONFIG")
        .env("CHIRAL_POLICY_PUBLIC_KEY", &policy_key)
        .stdin(Stdio::null())
        .args(["--check", "--version-policy", "policy.json", "--config"])
        .arg(&relative);
    let (success, stdout, stderr) = finish(command, &logs, "version-policy");
    assert!(success, "--version-policy: {stderr}");
    assert!(stdout.contains("validated; no services started"), "{stdout}");
    assert!(watermark.join(&policy_key).join("policy.json").is_file(), "--version-policy");
    assert!(!configured.join("policy.json").exists());
    assert_eq!(snapshot(&work), work_before, "--version-policy");
}
