//! Process tests of `chiral_daemon` with a development configuration whose CPU managed runtime's
//! `llama-server` is a full copy of this test binary, inventoried (`managed-engine-runtime`
//! requirements 2, 6 and 11, tasks 2.1, 2.3 and 3.2; `provider-daemon-lifecycle` requirement 11,
//! task 2.2).
//!
//! This target has `harness = false` (Cargo.toml): libtest would reject the launcher's
//! `llama-server` arguments, so `main` acts as the stub engine when it runs under that name: it
//! records its process id next to its key file and sleeps without answering health, or, when the
//! file `SERVES_HEALTH` is next to its key file, answers `GET /health` on its configured address,
//! so that the daemon starts and serves. Under any other name it runs the tests below in
//! sequence; a name given on the command line filters them.
use chiral_network::{
    engine_runtime::{ENGINE_VERSION, SOURCE_REVISION},
    network::{ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    protocol, wallet,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

const ENGINE: &str = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
const HELPER_PID: &str = "engine-helper.pid";
/// Beside the engine key file: the stub engine answers its health check.
const SERVES_HEALTH: &str = "engine-serves-health";
// Test-only credentials. The signing key has letters, so that letter case matters.
const SIGNING_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ADMIN_TOKEN: &str = "private-test-operator-token-32-bytes";
const RPC_BEARER: &str = "private-test-rpc-bearer-token-of-40-chars";
const ENGINE_TOKEN: &str = "e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7";

fn main() {
    let program = std::env::args_os().next().map(PathBuf::from);
    if program.as_deref().and_then(Path::file_name).is_some_and(|name| name == std::ffi::OsStr::new(ENGINE)) {
        return engine_stub();
    }
    // The libtest options that matter here; the values of the others are not filters.
    let (mut filters, mut skips, mut exact, mut list, mut ignored) = (Vec::new(), Vec::new(), false, false, false);
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--exact" => exact = true,
            "--list" => list = true,
            "--ignored" => ignored = true,
            "--skip" => skips.extend(arguments.next()),
            "--test-threads" | "--format" | "--color" | "--logfile" | "--shuffle-seed" | "-Z" => { arguments.next(); }
            option if option.starts_with('-') => {}
            filter => filters.push(filter.to_owned()),
        }
    }
    let selects = |name: &str, pattern: &String| if exact { name == pattern } else { name.contains(pattern.as_str()) };
    let mut tests: Vec<(&str, fn(&Runtime))> = vec![
        ("check_refuses_an_engine_key_that_repeats_another_credential", check_refuses_an_engine_key_that_repeats_another_credential),
        ("start_refuses_an_engine_key_that_repeats_another_credential_before_any_engine_starts", start_refuses_an_engine_key_that_repeats_another_credential_before_any_engine_starts),
        ("check_and_start_refuse_an_altered_inventory_before_any_engine_starts", check_and_start_refuse_an_altered_inventory_before_any_engine_starts),
        ("check_and_start_verify_every_managed_model", check_and_start_verify_every_managed_model),
    ];
    #[cfg(unix)]
    tests.extend([
        ("sigterm_during_engine_startup_stops_the_engine", sigterm_during_engine_startup_stops_the_engine as fn(&Runtime)),
        ("sigint_during_engine_startup_stops_the_engine", sigint_during_engine_startup_stops_the_engine),
        ("sighup_during_engine_startup_stops_the_engine", sighup_during_engine_startup_stops_the_engine),
        ("sigterm_while_serving_stops_the_engine_and_keeps_the_deployment", sigterm_while_serving_stops_the_engine_and_keeps_the_deployment),
        ("sighup_while_serving_stops_the_engine_and_keeps_the_deployment", sighup_while_serving_stops_the_engine_and_keeps_the_deployment),
        ("sighup_stays_ignored_when_the_daemon_starts_ignoring_it", sighup_stays_ignored_when_the_daemon_starts_ignoring_it),
    ]);
    // Nothing here is ignored; `--ignored` selects no test, as under libtest.
    tests.retain(|(name, _)| {
        !ignored && (filters.is_empty() || filters.iter().any(|f| selects(name, f))) && !skips.iter().any(|s| selects(name, s))
    });
    if list {
        for (name, _) in &tests { println!("{name}: test"); }
        return;
    }
    println!("\nrunning {} tests", tests.len());
    let runtime = (!tests.is_empty()).then(Runtime::new);
    let mut failed = Vec::new();
    for (name, test) in &tests {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(runtime.as_ref().unwrap())));
        println!("test {name} ... {}", if result.is_ok() { "ok" } else { "FAILED" });
        if result.is_err() { failed.push(*name); }
    }
    println!("\ntest result: {}. {} passed; {} failed", if failed.is_empty() { "ok" } else { "FAILED" }, tests.len() - failed.len(), failed.len());
    if !failed.is_empty() {
        println!("failures: {failed:?}");
        std::process::exit(101);
    }
}

/// The stub engine: answers nothing, so the daemon waits for its health until the startup timeout,
/// unless `SERVES_HEALTH` is beside its key file.
fn engine_stub() {
    let arguments: Vec<_> = std::env::args_os().collect();
    let value = |name: &str| arguments.iter().position(|a| a == name).and_then(|i| arguments.get(i + 1));
    let directory = value("--api-key-file").map(Path::new).and_then(Path::parent);
    if let Some(directory) = directory {
        let temporary = directory.join(format!("{HELPER_PID}.tmp"));
        if std::fs::write(&temporary, std::process::id().to_string()).is_ok() {
            let _ = std::fs::rename(&temporary, directory.join(HELPER_PID));
        }
    }
    if directory.is_some_and(|directory| directory.join(SERVES_HEALTH).exists()) {
        let host = value("--host").and_then(|host| host.to_str()).unwrap_or("127.0.0.1");
        let port = value("--port").and_then(|port| port.to_str()).and_then(|port| port.parse::<u16>().ok());
        if let Some(listener) = port.and_then(|port| std::net::TcpListener::bind((host, port)).ok()) {
            std::thread::spawn(move || serve_health(listener));
        }
    }
    // Bounded, so that a failed run cannot leave the stub behind for long.
    std::thread::sleep(Duration::from_secs(120));
}

/// Answers `GET /health` with 200 and anything else with 404, so the daemon's startup health
/// check passes and its readiness probes fail: the daemon serves with the model not ready.
fn serve_health(listener: std::net::TcpListener) {
    for mut stream in listener.incoming().flatten() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") && request.len() < 65536 {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => request.extend_from_slice(&buffer[..read]),
            }
        }
        let response: &[u8] = if request.starts_with(b"GET /health ") {
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}"
        } else {
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        };
        let _ = stream.write_all(response);
    }
}

/// One inventoried runtime and artifact shared by every case; the daemon only reads them.
struct Runtime {
    root: tempfile::TempDir,
    model: Value,
}

impl Runtime {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let runtime = root.path().join("runtime");
        let artifact = root.path().join("model");
        std::fs::create_dir(&runtime).unwrap();
        std::fs::create_dir(&artifact).unwrap();
        // A full copy, so that the launcher's native-header check passes and the copy runs.
        std::fs::copy(std::env::current_exe().unwrap(), runtime.join(ENGINE)).unwrap();
        let engine = std::fs::read(runtime.join(ENGINE)).unwrap();
        let platform = if cfg!(target_os = "macos") { "macos" } else { std::env::consts::OS };
        write_canonical(&runtime.join("chiral-runtime.json"), &json!({
            "schema": "chiral-llama-runtime-v1", "platform": platform,
            "architecture": std::env::consts::ARCH, "backend": "cpu",
            "engine_version": ENGINE_VERSION, "source_revision": SOURCE_REVISION, "executable": ENGINE,
            "files": { ENGINE: { "sha256": hex::encode(Sha256::digest(&engine)), "size_bytes": engine.len() } }
        }));
        let model = write_artifact(&artifact, "Q4_K_M", "ab", b"GGUF-test-inventory-bytes");
        Self { root, model }
    }

    /// A provider directory with its own credentials, data and ports. `engine_key` names the
    /// engine key file relative to it; the distinct engine token is always at `engine.token`.
    fn provider(&self, name: &str, engine_key: &str, engine_key_contents: Option<&str>) -> Provider {
        let directory = self.root.path().join(name);
        std::fs::create_dir(&directory).unwrap();
        let directory = directory.canonicalize().unwrap();
        wallet::save_private_key(&directory.join("wallet.key"), SIGNING_KEY).unwrap();
        protected(&directory.join("admin.token"), ADMIN_TOKEN);
        protected(&directory.join("rpc.token"), RPC_BEARER);
        protected(&directory.join("engine.token"), ENGINE_TOKEN);
        if let Some(contents) = engine_key_contents {
            protected(&directory.join(engine_key), contents);
        }
        let [public, admin, engine, rpc] = [(); 4].map(|_| free_port());
        let config = directory.join("provider.json");
        std::fs::write(&config, serde_json::to_vec_pretty(&json!({
            "network": {
                "identity": { "chain_id": ESPACE_TESTNET_CHAIN_ID, "genesis_hash": ESPACE_TESTNET_GENESIS,
                    "deployment_id": "daemon-engine-process-test" },
                // Nothing listens here: the chain is unavailable, which a development start allows.
                "rpc_url": format!("http://127.0.0.1:{rpc}"), "rpc_bearer_file": "rpc.token",
                "faucet_url": null, "explorer_url": null, "policy_url": null
            },
            "data_dir": "data", "signing_key_file": "wallet.key", "admin_token_file": "admin.token",
            "public_endpoint": format!("http://127.0.0.1:{public}"),
            "public_bind": format!("127.0.0.1:{public}"), "admin_bind": format!("127.0.0.1:{admin}"),
            "platform_wallet": "0x2222222222222222222222222222222222222222",
            "fee_bps": 50, "min_funding_wei": "1000",
            "models": [{
                "model": self.model,
                "price": { "input_wei_per_1k": "0", "output_wei_per_1k": "0" },
                "engine_url": format!("http://127.0.0.1:{engine}"), "engine_api_key_file": engine_key,
                "runtime": {
                    "runtime_dir": self.root.path().join("runtime"), "artifact_dir": self.root.path().join("model"),
                    "backend": "cpu", "device_ids": [], "memory_reserve_mib": 0, "gpu_layers": 0,
                    "split_mode": "none", "tensor_split": [], "cache_type_k": "f16", "cache_type_v": "f16",
                    "flash_attention": false, "batch_size": 128, "ubatch_size": 64, "startup_timeout_secs": 600
                },
                "max_concurrency": 2, "request_timeout_secs": 30, "qualification_evidence_file": null
            }],
            "dht": { "listen": "/ip4/127.0.0.1/tcp/0", "bootstrap": [], "identity_file": "dht.key" },
            "development": true
        })).unwrap()).unwrap();
        Provider { directory, config, public, engine }
    }
}

struct Provider {
    directory: PathBuf,
    config: PathBuf,
    #[cfg_attr(not(unix), allow(dead_code))]
    public: u16,
    #[cfg_attr(not(unix), allow(dead_code))]
    engine: u16,
}

impl Provider {
    fn daemon(&self, check: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_chiral_daemon"));
        command.env_remove("CHIRAL_PROVIDER_CONFIG").arg("--config").arg(&self.config);
        if check { command.arg("--check"); }
        command
    }
    /// The stub engine's process id, once a launched stub has recorded it.
    fn helper(&self) -> Option<u32> {
        std::fs::read_to_string(self.directory.join(HELPER_PID)).ok()?.trim().parse().ok()
    }
}

/// Writes an inventoried chat artifact of model `test/repo/model-<quantization>.gguf`, whose
/// revision repeats `revision` and whose GGUF holds `gguf`, into the existing `artifact`; returns
/// its ModelSpec.
fn write_artifact(artifact: &Path, quantization: &str, revision: &str, gguf: &[u8]) -> Value {
    let template = b"{{ messages }}";
    let file = format!("model-{quantization}.gguf");
    std::fs::write(artifact.join(&file), gguf).unwrap();
    std::fs::write(artifact.join("chiral-chat-template.jinja"), template).unwrap();
    let model = json!({
        "id": format!("test/repo/{file}"), "revision": revision.repeat(20),
        "tokenizer_revision": revision.repeat(20), "chat_template_sha256": hex::encode(Sha256::digest(template)),
        "quantization": quantization, "context_length": 512, "max_output_tokens": 64,
        "capabilities": ["chat", "completions"], "serving": null
    });
    write_canonical(&artifact.join("chiral-model.json"), &model);
    write_canonical(&artifact.join("chiral-artifact-sha256.json"), &json!({
        "schema": "chiral-gguf-artifact-v1",
        "model": { "repo_id": "test/repo", "revision": model["revision"], "license": "test" },
        "task": "generation", "quantization": quantization, "context_length": 512, "max_output_tokens": 64,
        "capabilities": model["capabilities"], "chat_template_name": "default", "model_file": file,
        "files": {
            file.as_str(): { "sha256": hex::encode(Sha256::digest(gguf)), "size_bytes": gguf.len(),
                "upstream_repo": "test/repo", "upstream_path": file, "upstream_digest": revision.repeat(32) },
            "chiral-chat-template.jinja": { "sha256": hex::encode(Sha256::digest(template)), "size_bytes": template.len(),
                "upstream_repo": null, "upstream_path": null, "upstream_digest": null }
        }
    }));
    model
}

fn write_canonical(path: &Path, value: &Value) {
    std::fs::write(path, protocol::canonical_json(value).unwrap()).unwrap();
}

fn protected(path: &Path, contents: &str) {
    wallet::create_secret_file(path).unwrap().write_all(contents.as_bytes()).unwrap();
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

/// The ways an engine key can repeat another credential (book line 474), as a file name
/// relative to the provider directory and, for a copy, its contents.
fn repeated_credentials() -> [(&'static str, &'static str, Option<String>); 4] {
    [
        ("signing key file", "wallet.key", None),
        ("0X upper-case signing key", "upper.key", Some(format!("0X{}", SIGNING_KEY.to_ascii_uppercase()))),
        ("administration token copy", "admin-copy.token", Some(ADMIN_TOKEN.into())),
        ("RPC bearer copy", "rpc-copy.token", Some(RPC_BEARER.into())),
    ]
}

fn assert_no_secret(case: &str, text: &str) {
    let text = text.to_ascii_lowercase();
    for secret in [SIGNING_KEY, ADMIN_TOKEN, RPC_BEARER, ENGINE_TOKEN] {
        assert!(!text.contains(&secret.to_ascii_lowercase()), "{case}: output contains a credential");
    }
}

fn describe(output: &Output) -> String {
    format!("{:?}\nstdout: {}\nstderr: {}", output.status, String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

/// Task 2.1: `chiral_daemon --check` refuses, without printing it, an engine key that repeats the
/// signing key (its file, or `0X` and upper case in another file), the administration token or
/// the RPC bearer, and accepts a distinct 64-character token.
fn check_refuses_an_engine_key_that_repeats_another_credential(runtime: &Runtime) {
    for (index, (case, file, contents)) in repeated_credentials().into_iter().enumerate() {
        let provider = runtime.provider(&format!("check-{index}"), file, contents.as_deref());
        let output = provider.daemon(true).output().unwrap();
        let report = describe(&output);
        assert!(!output.status.success(), "{case}: {report}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("must hold a separate credential"), "{case}: {report}");
        assert_no_secret(case, &report);
        assert!(provider.helper().is_none(), "{case}: an engine started");
    }
    let provider = runtime.provider("check-distinct", "engine.token", None);
    let output = provider.daemon(true).output().unwrap();
    let report = describe(&output);
    assert!(output.status.success(), "{report}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("validated; no services started"), "{report}");
    assert_no_secret("distinct engine token", &report);
}

/// Kills what a failed case left behind: the daemon and, by its recorded id, the stub engine.
struct Cleanup<'a> {
    daemon: Child,
    provider: &'a Provider,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        #[cfg(unix)]
        if let Some(pid) = self.provider.helper().filter(|pid| exists(*pid)) {
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
}

fn spawn(provider: &Provider) -> Cleanup<'_> {
    spawn_command(provider, provider.daemon(false))
}

fn spawn_command(provider: &Provider, mut command: Command) -> Cleanup<'_> {
    let log = |name: &str| std::fs::File::create(provider.directory.join(name)).unwrap();
    let daemon = command.stdin(Stdio::null()).stdout(log("daemon.stdout")).stderr(log("daemon.stderr")).spawn().unwrap();
    Cleanup { daemon, provider }
}

fn logs(provider: &Provider) -> String {
    let read = |name: &str| std::fs::read_to_string(provider.directory.join(name)).unwrap_or_default();
    format!("stdout: {}\nstderr: {}", read("daemon.stdout"), read("daemon.stderr"))
}

fn wait(daemon: &mut Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = daemon.try_wait().unwrap() { return Some(status); }
        if Instant::now() >= deadline { return None; }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Task 2.1: a daemon start with such an engine key exits nonzero, without printing it, before
/// any engine child exists, although the inventoried stub engine would otherwise start.
fn start_refuses_an_engine_key_that_repeats_another_credential_before_any_engine_starts(runtime: &Runtime) {
    for (index, (case, file, contents)) in repeated_credentials().into_iter().enumerate() {
        let provider = runtime.provider(&format!("start-{index}"), file, contents.as_deref());
        let mut run = spawn(&provider);
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            assert!(provider.helper().is_none(), "{case}: an engine child was started\n{}", logs(&provider));
            if let Some(status) = run.daemon.try_wait().unwrap() { break status; }
            assert!(Instant::now() < deadline, "{case}: the daemon kept starting\n{}", logs(&provider));
            std::thread::sleep(Duration::from_millis(50));
        };
        let report = logs(&provider);
        assert!(!status.success(), "{case}: {status:?}\n{report}");
        assert!(report.contains("must hold a separate credential"), "{case}: {report}");
        assert_no_secret(case, &report);
        assert!(provider.helper().is_none(), "{case}: an engine child was started\n{report}");
    }
}

/// Gives `provider` its own copy of the shared runtime and artifact, the engine hard-linked (so
/// that it is the same runnable stub), and points its configuration at them. `alter` then changes
/// the copy.
fn own_inventory(runtime: &Runtime, provider: &Provider, alter: fn(&Path, &Path)) {
    let (runtime_copy, artifact_copy) = (provider.directory.join("runtime"), provider.directory.join("model"));
    for (source, copy) in [(runtime.root.path().join("runtime"), &runtime_copy), (runtime.root.path().join("model"), &artifact_copy)] {
        std::fs::create_dir(copy).unwrap();
        for entry in std::fs::read_dir(&source).unwrap() {
            let path = entry.unwrap().path();
            let target = copy.join(path.file_name().unwrap());
            if path.file_name() == Some(std::ffi::OsStr::new(ENGINE)) { std::fs::hard_link(&path, &target).unwrap(); } else { std::fs::copy(&path, &target).unwrap(); }
        }
    }
    let mut config: Value = serde_json::from_slice(&std::fs::read(&provider.config).unwrap()).unwrap();
    config["models"][0]["runtime"]["runtime_dir"] = json!(runtime_copy);
    config["models"][0]["runtime"]["artifact_dir"] = json!(artifact_copy);
    std::fs::write(&provider.config, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    alter(&runtime_copy, &artifact_copy);
}

/// Rewrites the canonical JSON file at `path` after `change`.
fn edit_json(path: &Path, change: impl FnOnce(&mut Value)) {
    let mut value: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    change(&mut value);
    write_canonical(path, &value);
}

/// Task 3.2 (requirement 2), with task 3.1's floating identity: `chiral_daemon --check` and a
/// daemon start each refuse an altered inventory with its error and exit nonzero, and the
/// inventoried stub engine, which records its process id whenever it runs, never starts. The
/// control: the same copy unaltered validates, and a daemon start runs the stub.
fn check_and_start_refuse_an_altered_inventory_before_any_engine_starts(runtime: &Runtime) {
    let cases: [(&str, fn(&Path, &Path), &str); 5] = [
        ("source revision master", |runtime, _| edit_json(&runtime.join("chiral-runtime.json"), |m| m["source_revision"] = json!("master")),
            "runtime platform, backend, or pinned upstream identity mismatch"),
        ("the engine's digest", |runtime, _| edit_json(&runtime.join("chiral-runtime.json"), |m| m["files"][ENGINE]["sha256"] = json!("00".repeat(32))),
            "inventory file digest mismatch"),
        ("an unlisted library", |runtime, _| std::fs::write(runtime.join("unlisted.dll"), b"untrusted loader input").unwrap(), "unexpected file in immutable inventory"),
        ("one GGUF byte", |_, artifact| {
            let path = artifact.join("model-Q4_K_M.gguf");
            let mut bytes = std::fs::read(&path).unwrap();
            bytes[0] ^= 1;
            std::fs::write(path, bytes).unwrap();
        }, "inventory file digest mismatch"),
        ("the template removed", |_, artifact| std::fs::remove_file(artifact.join("chiral-chat-template.jinja")).unwrap(), "immutable inventory file is missing"),
    ];
    for (index, (case, alter, error)) in cases.into_iter().enumerate() {
        let provider = runtime.provider(&format!("altered-{index}"), "engine.token", None);
        own_inventory(runtime, &provider, alter);
        let output = provider.daemon(true).output().unwrap();
        let report = describe(&output);
        assert!(!output.status.success(), "{case}: --check succeeded\n{report}");
        assert!(String::from_utf8_lossy(&output.stderr).contains(error), "{case}: --check\n{report}");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("validated"), "{case}: --check\n{report}");
        assert_no_secret(case, &report);
        assert!(provider.helper().is_none(), "{case}: --check started the engine");

        let mut run = spawn(&provider);
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            assert!(provider.helper().is_none(), "{case}: an engine child was started\n{}", logs(&provider));
            if let Some(status) = run.daemon.try_wait().unwrap() { break status; }
            assert!(Instant::now() < deadline, "{case}: the daemon kept starting\n{}", logs(&provider));
            std::thread::sleep(Duration::from_millis(50));
        };
        let report = logs(&provider);
        assert!(!status.success(), "{case}: {status:?}\n{report}");
        assert!(report.contains(error), "{case}: {report}");
        assert!(!report.contains("Chiral provider public="), "{case}: the daemon served\n{report}");
        assert_no_secret(case, &report);
        assert!(provider.helper().is_none(), "{case}: an engine child was started\n{report}");
    }
    // The control: the same kind of copy, unaltered, validates, and a start runs the stub engine.
    let provider = runtime.provider("altered-control", "engine.token", None);
    own_inventory(runtime, &provider, |_, _| {});
    let output = provider.daemon(true).output().unwrap();
    assert!(output.status.success(), "the unaltered copy: {}", describe(&output));
    let mut run = spawn(&provider);
    let deadline = Instant::now() + Duration::from_secs(120);
    while provider.helper().is_none() {
        assert!(run.daemon.try_wait().unwrap().is_none(), "the daemon exited before its engine started\n{}", logs(&provider));
        assert!(Instant::now() < deadline, "the stub engine did not start\n{}", logs(&provider));
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Task 3.2 (requirement 2): verification runs for every managed model, not only the first. A
/// provider serves a second managed model, `test/repo/model-Q8_0.gguf`, with its own artifact.
/// Unaltered, `--check` validates both. With one byte of the second GGUF changed, `--check`
/// refuses with that error, and a daemon start launches and readies the first model's stub engine,
/// then refuses the second model, exits nonzero without serving, and leaves no engine running.
fn check_and_start_verify_every_managed_model(runtime: &Runtime) {
    let provider = runtime.provider("two-models", "engine.token", None);
    std::fs::write(provider.directory.join(SERVES_HEALTH), b"").unwrap();
    let artifact = provider.directory.join("model-q8");
    std::fs::create_dir(&artifact).unwrap();
    let second = write_artifact(&artifact, "Q8_0", "cd", b"GGUF-second-test-inventory-bytes");
    let mut config: Value = serde_json::from_slice(&std::fs::read(&provider.config).unwrap()).unwrap();
    let mut model = config["models"][0].clone();
    model["model"] = second;
    model["engine_url"] = json!(format!("http://127.0.0.1:{}", free_port()));
    model["runtime"]["artifact_dir"] = json!(artifact);
    config["models"].as_array_mut().unwrap().push(model);
    std::fs::write(&provider.config, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
    let output = provider.daemon(true).output().unwrap();
    assert!(output.status.success(), "two unaltered models: {}", describe(&output));

    let gguf = artifact.join("model-Q8_0.gguf");
    let mut bytes = std::fs::read(&gguf).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&gguf, bytes).unwrap();
    let output = provider.daemon(true).output().unwrap();
    let report = describe(&output);
    assert!(!output.status.success(), "--check succeeded\n{report}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("inventory file digest mismatch"), "--check\n{report}");
    assert!(provider.helper().is_none(), "--check started an engine");

    let mut run = spawn(&provider);
    let status = wait(&mut run.daemon, Duration::from_secs(120)).unwrap_or_else(|| panic!("the daemon kept running\n{}", logs(&provider)));
    let report = logs(&provider);
    assert!(!status.success(), "{status:?}\n{report}");
    assert!(report.contains("inventory file digest mismatch"), "{report}");
    assert!(!report.contains("Chiral provider public="), "the daemon served\n{report}");
    // The first model's engine had started; it does not outlive the refused start.
    let first = provider.helper().unwrap_or_else(|| panic!("the first model's engine never started\n{report}"));
    #[cfg(unix)]
    {
        let deadline = Instant::now() + Duration::from_secs(5);
        while exists(first) && Instant::now() < deadline { std::thread::sleep(Duration::from_millis(20)); }
        assert!(!exists(first), "the first model's engine outlived the refused start\n{report}");
    }
    #[cfg(not(unix))]
    let _ = first;
}

/// Whether a process exists and is not merely a zombie awaiting its reaper.
#[cfg(unix)]
fn exists(pid: u32) -> bool {
    if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 { return false; }
    #[cfg(target_os = "linux")]
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        if stat.rsplit_once(')').is_some_and(|(_, rest)| rest.trim_start().starts_with('Z')) { return false; }
    }
    true
}

/// Task 2.3 (requirement 11): a termination request while the engine loads makes the daemon exit
/// nonzero without serving, and the engine child no longer exists within five seconds.
#[cfg(unix)]
fn termination_during_engine_startup_stops_the_engine(runtime: &Runtime, signal: libc::c_int, name: &str) {
    let provider = runtime.provider(name, "engine.token", None);
    let mut run = spawn(&provider);
    let deadline = Instant::now() + Duration::from_secs(120);
    let helper = loop {
        if let Some(pid) = provider.helper() { break pid; }
        assert!(run.daemon.try_wait().unwrap().is_none(), "the daemon exited before its engine started\n{}", logs(&provider));
        assert!(Instant::now() < deadline, "the stub engine did not start\n{}", logs(&provider));
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(exists(helper));
    let signalled = Instant::now();
    assert_eq!(unsafe { libc::kill(run.daemon.id() as libc::pid_t, signal) }, 0);
    let status = wait(&mut run.daemon, Duration::from_secs(30)).unwrap_or_else(|| panic!("the daemon did not exit\n{}", logs(&provider)));
    let report = logs(&provider);
    // Exited by itself with a failure, not by the signal's default action.
    assert!(status.code().is_some_and(|code| code != 0), "{status:?}\n{report}");
    assert!(report.contains("termination requested during startup"), "{report}");
    assert!(!report.contains("Chiral provider public="), "the daemon served\n{report}");
    while exists(helper) && signalled.elapsed() < Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(20)); }
    assert!(!exists(helper), "the engine child outlived the daemon\n{report}");
}

#[cfg(unix)]
fn sigterm_during_engine_startup_stops_the_engine(runtime: &Runtime) {
    termination_during_engine_startup_stops_the_engine(runtime, libc::SIGTERM, "sigterm");
}

#[cfg(unix)]
fn sigint_during_engine_startup_stops_the_engine(runtime: &Runtime) {
    termination_during_engine_startup_stops_the_engine(runtime, libc::SIGINT, "sigint");
}

#[cfg(unix)]
fn sighup_during_engine_startup_stops_the_engine(runtime: &Runtime) {
    termination_during_engine_startup_stops_the_engine(runtime, libc::SIGHUP, "sighup");
}

/// `GET /healthz` on the daemon's public listener answers 200.
#[cfg(unix)]
fn healthz(port: u16) -> bool {
    let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    if stream.write_all(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").is_err() {
        return false;
    }
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response.starts_with("HTTP/1.1 200")
}

/// Waits until the daemon has printed its start line, then checks that `/healthz` answers 200:
/// the daemon serves. Returns the peer identity from the start line.
#[cfg(unix)]
fn served(provider: &Provider, run: &mut Cleanup<'_>, case: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(120);
    let peer = loop {
        let report = logs(provider);
        if let Some(line) = report.lines().find(|line| line.contains("Chiral provider public=")) {
            break line.rsplit_once("peer_id=").map(|(_, peer)| peer.trim().to_owned()).unwrap();
        }
        assert!(run.daemon.try_wait().unwrap().is_none(), "{case}: the daemon exited before serving\n{report}");
        assert!(Instant::now() < deadline, "{case}: the daemon did not serve\n{report}");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(healthz(provider.public), "{case}: /healthz did not answer\n{}", logs(provider));
    peer
}

/// The deployment's ledger: `data/<network>/<wallet>/<peer id>/ledger.sqlite3`.
#[cfg(unix)]
fn ledgers(provider: &Provider) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![provider.directory.join("data")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() { pending.push(path); } else if path.file_name() == Some(std::ffi::OsStr::new("ledger.sqlite3")) { found.push(path); }
        }
    }
    found
}

/// Task 2.2 (`provider-daemon-lifecycle` requirement 11, design code difference 2): a termination
/// request while the daemon serves, with a managed engine that passed its health check, takes the
/// shutdown path. The daemon exits by itself with status 0 within its shutdown bound (75 seconds
/// for admitted requests, then the engine stop), the engine process no longer exists and its port
/// is free, and the deployment's state is kept: the DHT identity is unchanged and the ledger is
/// still there, and a restart serves the same deployment (the same peer identity), which it then
/// leaves the same way. The `SIGTERM` case is the control: `SIGHUP` must exit like it.
#[cfg(unix)]
fn termination_while_serving_stops_the_engine_and_keeps_the_deployment(runtime: &Runtime, signal: libc::c_int, name: &str) {
    let provider = runtime.provider(name, "engine.token", None);
    std::fs::write(provider.directory.join(SERVES_HEALTH), b"").unwrap();
    let mut deployment = None;
    for start in ["start", "restart"] {
        let _ = std::fs::remove_file(provider.directory.join(HELPER_PID));
        let mut run = spawn(&provider);
        let peer = served(&provider, &mut run, start);
        let helper = provider.helper().unwrap_or_else(|| panic!("{start}: no engine\n{}", logs(&provider)));
        assert!(exists(helper), "{start}");
        let identity = std::fs::read(provider.directory.join("dht.key")).unwrap();
        let ledger = ledgers(&provider);
        assert_eq!(ledger.len(), 1, "{start}: {ledger:?}");
        assert!(ledger[0].to_string_lossy().contains(&peer), "{start}: {ledger:?} {peer}");
        let state = (peer, identity, ledger);
        if let Some(earlier) = &deployment {
            assert_eq!(earlier, &state, "{start}: the restart did not serve the same deployment");
        }

        let signalled = Instant::now();
        assert_eq!(unsafe { libc::kill(run.daemon.id() as libc::pid_t, signal) }, 0);
        let status = wait(&mut run.daemon, Duration::from_secs(90));
        let exited = signalled.elapsed();
        while exists(helper) && signalled.elapsed() < exited + Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(20)); }
        let engine_survived = exists(helper);
        let port_free = std::net::TcpListener::bind(("127.0.0.1", provider.engine)).is_ok();
        let report = format!("{start}: status {status:?} after {exited:?}, engine survived {engine_survived}, engine port free {port_free}\n{}", logs(&provider));
        // Exited by itself through the shutdown path, not by the signal's default action.
        let status = status.unwrap_or_else(|| panic!("the daemon did not exit\n{report}"));
        assert_eq!(status.code(), Some(0), "{report}");
        assert!(!engine_survived, "the engine child outlived the daemon\n{report}");
        assert!(port_free, "the engine port is still occupied\n{report}");
        assert_eq!(std::fs::read(provider.directory.join("dht.key")).unwrap(), state.1, "{report}");
        assert_eq!(ledgers(&provider), state.2, "{report}");
        deployment = Some(state);
    }
}

#[cfg(unix)]
fn sigterm_while_serving_stops_the_engine_and_keeps_the_deployment(runtime: &Runtime) {
    termination_while_serving_stops_the_engine_and_keeps_the_deployment(runtime, libc::SIGTERM, "sigterm-serving");
}

#[cfg(unix)]
fn sighup_while_serving_stops_the_engine_and_keeps_the_deployment(runtime: &Runtime) {
    termination_while_serving_stops_the_engine_and_keeps_the_deployment(runtime, libc::SIGHUP, "sighup-serving");
}

/// Task 2.2 and the design's Risks: a daemon started with `SIGHUP` ignored, as `nohup` starts it,
/// keeps ignoring it. Closing the terminal stops neither the daemon nor its engine; `SIGTERM` still
/// takes the shutdown path. Without the check, tokio's handler replaces the inherited disposition
/// and the hang-up ends the daemon through the shutdown path within a fraction of a second.
#[cfg(unix)]
fn sighup_stays_ignored_when_the_daemon_starts_ignoring_it(runtime: &Runtime) {
    use std::os::unix::process::CommandExt;
    let provider = runtime.provider("sighup-nohup", "engine.token", None);
    std::fs::write(provider.directory.join(SERVES_HEALTH), b"").unwrap();
    let mut command = provider.daemon(false);
    // SAFETY: `signal` is async-signal-safe, as `pre_exec` requires; `nohup` does the same.
    unsafe {
        command.pre_exec(|| {
            if libc::signal(libc::SIGHUP, libc::SIG_IGN) == libc::SIG_ERR {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut run = spawn_command(&provider, command);
    served(&provider, &mut run, "nohup");
    let helper = provider.helper().unwrap_or_else(|| panic!("no engine\n{}", logs(&provider)));
    assert_eq!(unsafe { libc::kill(run.daemon.id() as libc::pid_t, libc::SIGHUP) }, 0);
    // The SIGHUP and SIGTERM serving cases exit within a fraction of a second of the signal.
    std::thread::sleep(Duration::from_secs(2));
    let status = run.daemon.try_wait().unwrap();
    assert!(status.is_none(), "the daemon stopped on a hang-up it started ignoring: {status:?}\n{}", logs(&provider));
    assert!(healthz(provider.public), "/healthz stopped answering after the hang-up\n{}", logs(&provider));
    assert!(exists(helper), "the engine stopped after the hang-up\n{}", logs(&provider));

    let signalled = Instant::now();
    assert_eq!(unsafe { libc::kill(run.daemon.id() as libc::pid_t, libc::SIGTERM) }, 0);
    let status = wait(&mut run.daemon, Duration::from_secs(90));
    while exists(helper) && signalled.elapsed() < Duration::from_secs(95) { std::thread::sleep(Duration::from_millis(20)); }
    let report = format!("status {status:?}, engine survived {}\n{}", exists(helper), logs(&provider));
    assert_eq!(status.and_then(|status| status.code()), Some(0), "{report}");
    assert!(!exists(helper), "the engine child outlived the daemon\n{report}");
}
