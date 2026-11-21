//! Process tests of `chiral_daemon` with a development configuration whose CPU managed runtime's
//! `llama-server` is a full copy of this test binary, inventoried (`managed-engine-runtime`
//! requirements 2, 6, 9 and 11, tasks 2.1, 2.3, 3.2, 3.9 and 3.11; `provider-daemon-lifecycle`
//! requirement 11, task 2.2), and the process-level acceptance evidence of `provider-daemon-lifecycle`
//! requirements 7 to 12, tasks 3.7 to 3.12 (module `lifecycle`, whose stub chat and embedding
//! engines and chain RPC this test process serves itself), and, on macOS, the process-creation
//! evidence of `application-startup-boundaries` task 3.1: the daemon, started suspended and
//! watched by kqueue (`support/process_observer.rs`), creates no process with an external engine
//! and only its managed engine with the CPU runtime, and reaches only its configured chain RPC.
//!
//! This target has `harness = false` (Cargo.toml): libtest would reject the launcher's
//! `llama-server` arguments, so `main` acts as the stub engine when it runs under that name: it
//! records its process id next to its key file, and appends it to the log of engine starts there,
//! and sleeps without answering health; when the file `SERVES_HEALTH` is next to its key file, it
//! also answers `GET /health` on its configured address, so that the daemon starts and serves;
//! when the file `EXITS_AT_ONCE` is there, it exits at once instead. Under any other name it runs
//! the tests below in sequence; a name given on the command line filters them.
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

#[path = "support/process_observer.rs"]
mod process_observer;

const ENGINE: &str = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
const HELPER_PID: &str = "engine-helper.pid";
/// Beside the engine key file: the stub engine answers its health check.
const SERVES_HEALTH: &str = "engine-serves-health";
/// Beside the engine key file: the stub engine exits at once, as an engine that cannot load does.
const EXITS_AT_ONCE: &str = "engine-exits-at-once";
/// Beside the engine key file: one line, the process id, per start of the stub engine.
const ENGINE_STARTS: &str = "engine-starts.log";
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
        ("an_engine_that_exits_before_readiness_fails_the_start_without_a_second_launch", an_engine_that_exits_before_readiness_fails_the_start_without_a_second_launch),
    ];
    #[cfg(unix)]
    tests.extend([
        ("sigterm_during_engine_startup_stops_the_engine", sigterm_during_engine_startup_stops_the_engine as fn(&Runtime)),
        ("sigint_during_engine_startup_stops_the_engine", sigint_during_engine_startup_stops_the_engine),
        ("sighup_during_engine_startup_stops_the_engine", sighup_during_engine_startup_stops_the_engine),
        ("sigterm_while_serving_stops_the_engine_and_keeps_the_deployment", sigterm_while_serving_stops_the_engine_and_keeps_the_deployment),
        ("sighup_while_serving_stops_the_engine_and_keeps_the_deployment", sighup_while_serving_stops_the_engine_and_keeps_the_deployment),
        ("sighup_stays_ignored_when_the_daemon_starts_ignoring_it", sighup_stays_ignored_when_the_daemon_starts_ignoring_it),
        ("a_rejected_chain_identity_stops_the_daemon_before_it_serves_or_starts_an_engine", lifecycle::a_rejected_chain_identity_stops_the_daemon_before_it_serves_or_starts_an_engine),
        ("a_second_daemon_for_a_running_deployment_is_refused_before_it_joins_or_starts_an_engine", lifecycle::a_second_daemon_for_a_running_deployment_is_refused_before_it_joins_or_starts_an_engine),
        ("legacy_wallet_chain_and_drive_data_stays_in_place_through_start_serve_and_shutdown", lifecycle::legacy_wallet_chain_and_drive_data_stays_in_place_through_start_serve_and_shutdown),
        ("editing_the_configuration_of_a_running_daemon_changes_nothing_until_it_restarts", lifecycle::editing_the_configuration_of_a_running_daemon_changes_nothing_until_it_restarts),
        ("sigterm_with_an_admitted_stream_refuses_new_requests_and_exits_once_it_settles", lifecycle::sigterm_with_an_admitted_stream_refuses_new_requests_and_exits_once_it_settles),
        ("a_stream_that_outlasts_the_shutdown_bound_is_released_under_unknown_usage", lifecycle::a_stream_that_outlasts_the_shutdown_bound_is_released_under_unknown_usage),
        ("prompts_completions_and_credentials_reach_no_output_administration_answer_or_deployment_file", lifecycle::prompts_completions_and_credentials_reach_no_output_administration_answer_or_deployment_file),
    ]);
    // `application-startup-boundaries` tasks 3.1: the daemon's process creation, observed by
    // kqueue (macOS; the Linux `/proc` observation is still open).
    #[cfg(target_os = "macos")]
    tests.extend([
        ("an_unmanaged_daemon_starts_no_process_and_reaches_only_its_configured_chain_rpc", lifecycle::an_unmanaged_daemon_starts_no_process_and_reaches_only_its_configured_chain_rpc as fn(&Runtime)),
        ("a_managed_daemon_starts_only_its_engine_through_start_serve_and_shutdown", lifecycle::a_managed_daemon_starts_only_its_engine_through_start_serve_and_shutdown),
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
        if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(directory.join(ENGINE_STARTS)) {
            let _ = writeln!(log, "{}", std::process::id());
        }
        if directory.join(EXITS_AT_ONCE).exists() {
            std::process::exit(3);
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
        Provider { directory, config, public, admin, engine }
    }
}

struct Provider {
    directory: PathBuf,
    config: PathBuf,
    #[cfg_attr(not(unix), allow(dead_code))]
    public: u16,
    #[cfg_attr(not(unix), allow(dead_code))]
    admin: u16,
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
    /// The process ids of the stub engine's starts, in order.
    fn engine_starts(&self) -> Vec<String> {
        std::fs::read_to_string(self.directory.join(ENGINE_STARTS)).unwrap_or_default().lines().map(str::to_owned).collect()
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
        kill_stub_engines(self.provider);
    }
}

/// Kills the stub engines that `provider`'s daemons started: the last one by its recorded id
/// and, by the log of engine starts, every other one, as when two daemons ran for it; only a
/// process whose command line names this provider's directory, so that a reused process id
/// is never signalled.
#[cfg_attr(not(unix), allow(unused_variables))]
fn kill_stub_engines(provider: &Provider) {
    #[cfg(unix)]
    if let Some(pid) = provider.helper().filter(|pid| exists(*pid)) {
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    }
    #[cfg(unix)]
    for pid in provider.engine_starts().iter().filter_map(|pid| pid.parse::<u32>().ok()).filter(|pid| exists(*pid)) {
        let command = Command::new("ps").args(["-ww", "-o", "command=", "-p", &pid.to_string()]).output();
        if command.is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains(&*provider.directory.to_string_lossy())) {
            unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
        }
    }
}

fn spawn(provider: &Provider) -> Cleanup<'_> {
    spawn_command(provider, provider.daemon(false))
}

fn spawn_command(provider: &Provider, command: Command) -> Cleanup<'_> {
    spawn_named(provider, command, "daemon")
}

/// Spawns `command` with its standard output and error in `<name>.stdout` and `<name>.stderr` of
/// the provider directory.
fn spawn_named<'a>(provider: &'a Provider, mut command: Command, name: &str) -> Cleanup<'a> {
    let log = |stream: &str| std::fs::File::create(provider.directory.join(format!("{name}.{stream}"))).unwrap();
    let daemon = command.stdin(Stdio::null()).stdout(log("stdout")).stderr(log("stderr")).spawn().unwrap();
    Cleanup { daemon, provider }
}

fn logs(provider: &Provider) -> String {
    logs_named(provider, "daemon")
}

/// The standard output and error of the daemon that `spawn_named` started as `name`.
fn logs_named(provider: &Provider, name: &str) -> String {
    let read = |stream: &str| std::fs::read_to_string(provider.directory.join(format!("{name}.{stream}"))).unwrap_or_default();
    format!("stdout: {}\nstderr: {}", read("stdout"), read("stderr"))
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

/// Task 3.9 (requirement 9): an engine that exits before its health check succeeds fails the
/// daemon's start, and nothing starts it again. The stub engine, which logs each of its starts,
/// exits at once; its endpoint never answers. With a startup timeout of 600 seconds, the daemon
/// exits nonzero with the exited-before-readiness error, without serving, after exactly one start
/// of the engine. `--check` started none.
fn an_engine_that_exits_before_readiness_fails_the_start_without_a_second_launch(runtime: &Runtime) {
    let provider = runtime.provider("exits-at-once", "engine.token", None);
    std::fs::write(provider.directory.join(EXITS_AT_ONCE), b"").unwrap();
    let output = provider.daemon(true).output().unwrap();
    assert!(output.status.success(), "{}", describe(&output));
    assert!(provider.engine_starts().is_empty(), "--check started the engine");
    let mut run = spawn(&provider);
    let status = wait(&mut run.daemon, Duration::from_secs(120)).unwrap_or_else(|| panic!("the daemon kept starting\n{}", logs(&provider)));
    let report = logs(&provider);
    assert!(!status.success(), "{status:?}\n{report}");
    assert!(report.contains("managed engine exited before readiness"), "{report}");
    assert!(!report.contains("Chiral provider public="), "the daemon served\n{report}");
    let starts = provider.engine_starts();
    assert_eq!(starts.len(), 1, "engine starts {starts:?}\n{report}");
    assert_eq!(provider.helper().map(|pid| pid.to_string()).as_ref(), starts.first(), "{report}");
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
    served_named(provider, run, case, "daemon")
}

/// `served` for the daemon that `spawn_named` started as `name`, whose public listener is the one
/// of `provider` unless it has its own configuration; `/healthz` is checked on `provider.public`
/// only for the daemon named `daemon`.
#[cfg(unix)]
fn served_named(provider: &Provider, run: &mut Cleanup<'_>, case: &str, name: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(120);
    let peer = loop {
        let report = logs_named(provider, name);
        if let Some(line) = report.lines().find(|line| line.contains("Chiral provider public=")) {
            break line.rsplit_once("peer_id=").map(|(_, peer)| peer.trim().to_owned()).unwrap();
        }
        assert!(run.daemon.try_wait().unwrap().is_none(), "{case}: the daemon exited before serving\n{report}");
        assert!(Instant::now() < deadline, "{case}: the daemon did not serve\n{report}");
        std::thread::sleep(Duration::from_millis(50));
    };
    if name == "daemon" {
        assert!(healthz(provider.public), "{case}: /healthz did not answer\n{}", logs(provider));
    }
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
/// `managed-engine-runtime` task 3.11 (requirement 11, *Normal shutdown stops the engine*): the
/// engine process no longer exists at the moment the daemon has exited, observed before the
/// daemon is reaped, so the daemon stopped and reaped it first.
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
        let engine_at_exit = engine_existed_at_exit(&run.daemon, helper, Duration::from_secs(90));
        let status = wait(&mut run.daemon, Duration::from_secs(5));
        let exited = signalled.elapsed();
        while exists(helper) && signalled.elapsed() < exited + Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(20)); }
        let engine_survived = exists(helper);
        let port_free = std::net::TcpListener::bind(("127.0.0.1", provider.engine)).is_ok();
        let report = format!("{start}: status {status:?} after {exited:?}, engine existed at the daemon's exit {engine_at_exit:?}, engine survived {engine_survived}, engine port free {port_free}\n{}", logs(&provider));
        // Exited by itself through the shutdown path, not by the signal's default action.
        let status = status.unwrap_or_else(|| panic!("the daemon did not exit\n{report}"));
        assert_eq!(status.code(), Some(0), "{report}");
        assert_eq!(engine_at_exit, Some(false), "the engine process still existed when the daemon exited\n{report}");
        assert!(!engine_survived, "the engine child outlived the daemon\n{report}");
        assert!(port_free, "the engine port is still occupied\n{report}");
        assert_eq!(std::fs::read(provider.directory.join("dht.key")).unwrap(), state.1, "{report}");
        assert_eq!(ledgers(&provider), state.2, "{report}");
        deployment = Some(state);
    }
}

/// Waits, at most `timeout`, until `daemon` has exited, without reaping it, and reports whether
/// process `engine` existed at that moment (`None` when the daemon did not exit in time). The
/// daemon is then a zombie, so its exit is complete: an engine that it killed but did not reap
/// would still exist, as a zombie awaiting its new parent, and one it did not kill, alive.
#[cfg(unix)]
fn engine_existed_at_exit(daemon: &Child, engine: u32, timeout: Duration) -> Option<bool> {
    let pid = daemon.id();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let waited = loop {
            // SAFETY: an all-zero `siginfo_t` is valid output storage for `waitid`, which with
            // `WNOWAIT` leaves the child waitable for the reaping `wait` that follows.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let waited = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT) };
            if waited == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted { break waited; }
        };
        let _ = sender.send((waited == 0).then(|| exists(engine)));
    });
    receiver.recv_timeout(timeout).ok().flatten()
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

/// `provider-daemon-lifecycle` acceptance evidence at process level, tasks 3.7 to 3.12. Most cases
/// serve the development chat model `chat_model()` from an operator-owned stub engine served by
/// this test process (`engine_stub`), so that the daemon starts at once and serves inference; the
/// chain RPC is a stub here too (`chain_stub`) or absent. Accounts are free accounts opened in the
/// deployment's ledger before the daemon starts (`seed_free_account`).
#[cfg(unix)]
mod lifecycle {
    use super::*;
    use std::net::TcpStream;
    use std::sync::{atomic::{AtomicBool, Ordering}, Arc};

    const CHAT_MODEL: &str = "test/repo/model-Q4_K_M.gguf";
    const CHAT_TEMPLATE: &str = "{{ messages }}";
    /// In the prompts of task 3.12's requests; the stub engine completes such a prompt with
    /// `COMPLETION_MARKER`.
    const PROMPT_MARKER: &str = "prompt-marker-4b1f9e27c3";
    const COMPLETION_MARKER: &str = "completion-marker-8d2a6c15f0";
    /// In the arguments of the tool call with which the stub engine answers a prompt containing
    /// `PROMPT_MARKER` that offers tools.
    const TOOL_MARKER: &str = "tool-argument-marker-61e0d94a7b";
    /// The value of every float embedding that the stub embedding engine returns.
    const VECTOR_MARKER: &str = "0.6180339887";
    const EMBEDDING_MODEL: &str = "test/repo/embedding-Q8_0.gguf";
    /// The daemon's shutdown bound for admitted requests (chiral_daemon.rs), which task 3.11 waits.
    const SHUTDOWN_BOUND: Duration = Duration::from_secs(75);

    /// The development chat model of the operator-owned stub engine.
    fn chat_model() -> Value {
        json!({"id": CHAT_MODEL, "revision": "ab".repeat(20), "tokenizer_revision": "ab".repeat(20),
            "chat_template_sha256": hex::encode(Sha256::digest(CHAT_TEMPLATE)), "quantization": "Q4_K_M",
            "context_length": 512, "max_output_tokens": 64, "capabilities": ["chat"], "serving": null})
    }

    /// The development embedding model of the stub embedding engine (`embedding_stub`).
    fn embedding_model() -> Value {
        json!({"id": EMBEDDING_MODEL, "revision": "ef".repeat(20), "tokenizer_revision": "ef".repeat(20),
            "chat_template_sha256": "00".repeat(32), "quantization": "Q8_0", "context_length": 512,
            "max_output_tokens": 0, "capabilities": ["embeddings"], "serving": null})
    }

    /// The operator-owned engine of `embedding_model()` with one slot: two tokens per input, and
    /// one-dimensional embeddings whose float value is `VECTOR_MARKER`.
    fn embedding_stub() -> axum::Router {
        use axum::{routing::{get, post}, Json};
        axum::Router::new()
            .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
            .route("/v1/models", get(|| async { Json(json!({"data": [{"id": EMBEDDING_MODEL}]})) }))
            .route("/props", get(|| async { Json(json!({"build_info": "b1-cd959ba", "total_slots": 1,
                "model_alias": EMBEDDING_MODEL, "default_generation_settings": {"n_ctx": 512}})) }))
            .route("/tokenize", post(|| async { Json(json!({"tokens": [1, 2]})) }))
            .route("/v1/embeddings", post(|Json(request): Json<Value>| async move {
                let inputs = request["input"].as_array().cloned().unwrap_or_default();
                let tokens: usize = inputs.iter().map(|input| input.as_array().map_or(0, Vec::len)).sum();
                // 0.5 as one little-endian float32 for base64; `VECTOR_MARKER` as a float.
                let embedding = if request["encoding_format"] == "base64" { json!("AAAAPw==") }
                    else { json!([VECTOR_MARKER.parse::<f64>().unwrap()]) };
                let data: Vec<_> = (0..inputs.len()).map(|index| json!({"index": index, "object": "embedding",
                    "embedding": embedding})).collect();
                Json(json!({"model": request["model"], "object": "list", "data": data,
                    "usage": {"prompt_tokens": tokens, "total_tokens": tokens}}))
            }))
    }

    /// Stub services that the daemon calls, served by this test process on a runtime that ends with
    /// this value.
    struct Services(tokio::runtime::Runtime);

    impl Services {
        fn new() -> Self {
            Self(tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap())
        }
        /// Serves `router` on a free loopback port, which it returns.
        fn serve(&self, router: axum::Router) -> u16 {
            self.0.block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let port = listener.local_addr().unwrap().port();
                tokio::spawn(async move {
                    let _ = axum::serve(listener, router).await;
                });
                port
            })
        }
        /// Whether `watch` holds a value that satisfies `reached` within 30 seconds.
        fn reached<T>(&self, watch: &tokio::sync::watch::Sender<T>, reached: impl FnMut(&T) -> bool) -> bool {
            let mut receiver = watch.subscribe();
            self.0.block_on(async {
                matches!(tokio::time::timeout(Duration::from_secs(30), async { receiver.wait_for(reached).await.is_ok() }).await, Ok(true))
            })
        }
    }

    /// Holds each streamed chat request whose prompt contains `held` open after its first token,
    /// until `release`; `entered` counts the requests it has held.
    #[derive(Clone)]
    struct Held {
        entered: Arc<tokio::sync::watch::Sender<usize>>,
        open: Arc<tokio::sync::watch::Sender<bool>>,
    }

    impl Held {
        fn new() -> Self {
            Self { entered: Arc::new(tokio::sync::watch::channel(0).0), open: Arc::new(tokio::sync::watch::channel(false).0) }
        }
        fn release(&self) {
            self.open.send_replace(true);
        }
    }

    /// The operator-owned engine of `chat_model()` with `slots` slots. It passes the daemon's
    /// readiness probes and answers each chat request with the token `OK` and exact usage (two
    /// prompt tokens, one completion token). A prompt containing `PROMPT_MARKER` is completed with
    /// `COMPLETION_MARKER` as well; one containing `fail` fails with an error that repeats the
    /// prompt; a streamed one containing `held` is held open after its first token (`Held`). A
    /// request offering tools is answered with a call of `ping`, with no arguments, or with
    /// `TOOL_MARKER` in them when the prompt contains `PROMPT_MARKER`.
    fn engine_stub(slots: u32, held: Held) -> axum::Router {
        engine_stub_of(Arc::new(std::sync::atomic::AtomicU32::new(slots)), held)
    }

    /// `engine_stub` whose number of slots is what `slots` holds when the daemon asks.
    fn engine_stub_of(slots: Arc<std::sync::atomic::AtomicU32>, held: Held) -> axum::Router {
        use axum::{routing::{get, post}, Json};
        let props = move || json!({"build_info": "b1-cd959ba", "total_slots": slots.load(Ordering::Acquire),
            "model_alias": CHAT_MODEL, "default_generation_settings": {"n_ctx": 512}, "chat_template": CHAT_TEMPLATE});
        axum::Router::new()
            .route("/health", get(|| async { Json(json!({"status": "ok"})) }))
            .route("/v1/models", get(|| async { Json(json!({"data": [{"id": CHAT_MODEL}]})) }))
            .route("/props", get(move || async move { Json(props()) }))
            .route("/apply-template", post(|| async { Json(json!({"prompt": "rendered"})) }))
            .route("/tokenize", post(|| async { Json(json!({"tokens": [1, 2]})) }))
            .route("/v1/chat/completions", post(move |Json(request): Json<Value>| engine_answer(request, held.clone())))
    }

    async fn engine_answer(request: Value, held: Held) -> axum::response::Response {
        use axum::{http::{header, StatusCode}, response::IntoResponse, Json};
        use futures_util::StreamExt;
        let prompt = request["messages"][0]["content"].as_str().unwrap_or_default().to_owned();
        if prompt.contains("fail") {
            return (StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": {"message": format!("engine failure on the prompt: {prompt}")}}))).into_response();
        }
        let content = if prompt.contains(PROMPT_MARKER) { format!("OK {COMPLETION_MARKER}") } else { "OK".to_owned() };
        let usage = json!({"prompt_tokens": 2, "completion_tokens": 1, "total_tokens": 3});
        let logprobs = json!({"content": [{"id": 7, "token": "OK", "bytes": [79, 75], "logprob": -0.1,
            "top_logprobs": [{"token": "OK", "logprob": -0.1}]}]});
        if request.get("tools").is_some() {
            let arguments = if prompt.contains(PROMPT_MARKER) { json!({"query": TOOL_MARKER}).to_string() } else { "{}".to_owned() };
            let call = json!({"id": "call-1", "type": "function", "function": {"name": "ping", "arguments": arguments}});
            if request["stream"] != true {
                return Json(json!({"id": "stub", "object": "chat.completion", "model": request["model"],
                    "choices": [{"index": 0, "message": {"role": "assistant", "content": null, "tool_calls": [call]},
                        "finish_reason": "tool_calls"}], "usage": usage})).into_response();
            }
            let mut streamed = call;
            streamed["index"] = json!(0);
            let chunk = |choices: Value| json!({"id": "stub", "object": "chat.completion.chunk", "model": request["model"],
                "created": 1, "choices": choices});
            let call = chunk(json!([{"index": 0, "delta": {"role": "assistant", "tool_calls": [streamed]}, "finish_reason": null,
                "logprobs": logprobs}]));
            let finish = chunk(json!([{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]));
            let mut last = chunk(json!([]));
            last["usage"] = usage;
            return ([(header::CONTENT_TYPE, "text/event-stream")],
                format!("data: {call}\n\ndata: {finish}\n\ndata: {last}\n\ndata: [DONE]\n\n")).into_response();
        }
        if request["stream"] != true {
            return Json(json!({"id": "stub", "object": "chat.completion", "model": request["model"],
                "choices": [{"index": 0, "message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
                "usage": usage})).into_response();
        }
        let chunk = |choices: Value| json!({"id": "stub", "object": "chat.completion.chunk", "model": request["model"],
            "created": 1, "choices": choices});
        let token = chunk(json!([{"index": 0, "delta": {"role": "assistant", "content": content}, "finish_reason": null,
            "logprobs": logprobs}]));
        let finish = chunk(json!([{"index": 0, "delta": {}, "finish_reason": "stop"}]));
        let mut last = chunk(json!([]));
        last["usage"] = usage;
        let first = axum::body::Bytes::from(format!("data: {token}\n\n"));
        let rest = axum::body::Bytes::from(format!("data: {finish}\n\ndata: {last}\n\ndata: [DONE]\n\n"));
        let held = prompt.contains("held").then_some(held);
        let body = futures_util::stream::iter([Ok::<_, std::convert::Infallible>(first)])
            .chain(futures_util::stream::once(async move {
                if let Some(held) = held {
                    held.entered.send_modify(|count| *count += 1);
                    let _ = held.open.subscribe().wait_for(|open| *open).await;
                }
                Ok(rest)
            }));
        ([(header::CONTENT_TYPE, "text/event-stream")], axum::body::Body::from_stream(body)).into_response()
    }

    /// Holds the first request that reaches it, when armed, until `release`.
    #[derive(Clone)]
    struct Gate {
        armed: Arc<AtomicBool>,
        entered: Arc<tokio::sync::watch::Sender<bool>>,
        open: Arc<tokio::sync::watch::Sender<bool>>,
    }

    impl Gate {
        fn new(armed: bool) -> Self {
            Self { armed: Arc::new(AtomicBool::new(armed)), entered: Arc::new(tokio::sync::watch::channel(false).0),
                open: Arc::new(tokio::sync::watch::channel(false).0) }
        }
        async fn pass(&self) {
            if self.armed.swap(false, Ordering::AcqRel) {
                self.entered.send_replace(true);
                let _ = self.open.subscribe().wait_for(|open| *open).await;
            }
        }
        fn release(&self) {
            self.open.send_replace(true);
        }
    }

    /// A chain RPC that reports chain ID `chain_id` and, for block 0, `genesis`, and a current
    /// eSpace `safe` frontier, as `app::tests::ready_chain` does; `gate` holds its first request.
    fn chain_stub(chain_id: &'static str, genesis: String, gate: Gate) -> axum::Router {
        use axum::{http::StatusCode, response::IntoResponse, Json};
        axum::Router::new().fallback(move |Json(request): Json<Value>| {
            let (gate, genesis) = (gate.clone(), genesis.clone());
            async move {
                gate.pass().await;
                match chain_answer(chain_id, &genesis, &request) {
                    Some(answer) => Json(answer).into_response(),
                    None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
                }
            }
        })
    }

    /// The answer of `chain_stub`'s chain to the JSON-RPC `request`; `None` for a method or
    /// block it does not serve.
    fn chain_answer(chain_id: &str, genesis: &str, request: &Value) -> Option<Value> {
        let now = protocol::now().unwrap();
        let block = |height: u64, hash: String| json!({"number": format!("0x{height:x}"), "hash": hash,
            "timestamp": format!("0x{:x}", now - (12 - height))});
        let result = match (request["method"].as_str(), request["params"][0].as_str()) {
            (Some("eth_chainId"), _) => json!(chain_id),
            (Some("eth_syncing"), _) => json!(false),
            (Some("eth_getBlockByNumber"), Some("0x0")) => block(0, genesis.to_owned()),
            (Some("eth_getBlockByNumber"), Some("latest")) => block(12, format!("0x{}", "55".repeat(32))),
            (Some("eth_getBlockByNumber"), Some("safe")) => block(10, format!("0x{}", "56".repeat(32))),
            (Some("eth_getBlockByNumber"), Some("finalized")) => block(6, format!("0x{}", "57".repeat(32))),
            _ => return None,
        };
        Some(json!({"jsonrpc": "2.0", "id": request["id"], "result": result}))
    }

    /// Serves `provider`'s only model as `chat_model()` from the operator-owned stub engine on
    /// `engine`, with `slots` slots, a request timeout of 600 seconds (beyond the shutdown bound) and
    /// a nonzero price; without a managed runtime the daemon starts at once.
    fn unmanaged(provider: &Provider, engine: u16, slots: u32) {
        edit_json(&provider.config, |config| {
            let deployment = &mut config["models"][0];
            deployment["model"] = chat_model();
            deployment["runtime"] = Value::Null;
            deployment["engine_url"] = json!(format!("http://127.0.0.1:{engine}"));
            deployment["max_concurrency"] = json!(slots);
            deployment["request_timeout_secs"] = json!(600);
            deployment["price"] = json!({"input_wei_per_1k": "1000", "output_wei_per_1k": "2000"});
        });
    }

    /// Points `provider`'s chain RPC at `port`.
    fn chain_at(provider: &Provider, port: u16) {
        edit_json(&provider.config, |config| config["network"]["rpc_url"] = json!(format!("http://127.0.0.1:{port}")));
    }

    /// Opens a free account of the configured model, for at most `concurrency` requests at a time
    /// and owned by the wallet of signing key `consumer` repeated, in the ledger of `provider`'s
    /// deployment while no daemon runs, as an earlier daemon would have: its contract identifier
    /// and bearer.
    fn seed_free_account(provider: &Provider, consumer: &str, concurrency: u32) -> (String, String) {
        seed_free_account_of(provider, consumer, concurrency, 0)
    }

    /// `seed_free_account` for the configured model at `index`.
    fn seed_free_account_of(provider: &Provider, consumer: &str, concurrency: u32, index: usize) -> (String, String) {
        use chiral_network::{billing_store::BillingStore, dht::PeerIdentity, provider_config::ProviderConfig};
        let config = ProviderConfig::load(&provider.config).unwrap();
        let peer = PeerIdentity::load(&config.dht.identity_file).unwrap().peer_id();
        let provider_wallet = wallet::address_from_private_key(SIGNING_KEY).unwrap();
        let deployment = config.runtime_data_dir().unwrap().join(&provider_wallet).join(peer);
        wallet::secure_directory(&deployment).unwrap();
        let ledger = BillingStore::open(&deployment.join("ledger.sqlite3"), &config.network.identity).unwrap();
        let now = protocol::now().unwrap();
        let quote = protocol::sign(protocol::QUOTE_DOMAIN, protocol::Quote {
            id: protocol::random_id(), network: config.network.identity.clone(), offer_id: protocol::random_id(),
            provider_wallet, consumer_wallet: wallet::address_from_private_key(&consumer.repeat(32)).unwrap(),
            model: config.models[index].model.clone(),
            price: protocol::PriceSchedule { input_wei_per_1k: "0".into(), output_wei_per_1k: "0".into() },
            funding_wei: "0".into(), platform_wallet: config.platform_wallet.clone(), fee_bps: 50,
            max_concurrency: concurrency, nonce: protocol::random_id(), issued_at: now, expires_at: now + 300,
        }, SIGNING_KEY).unwrap();
        ledger.store_quote(&quote, now).unwrap();
        let bearer = ledger.open_account(&quote, None, now).unwrap().credential.unwrap().bearer;
        (quote.payload.id, bearer)
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|window| window == needle)
    }

    /// Sends one HTTP/1.1 request with `Connection: close` to the listener on `port`: the
    /// connection, from which the answer is read.
    fn send(port: u16, method: &str, path: &str, headers: &[(&str, String)], body: Option<&Value>) -> std::io::Result<TcpStream> {
        let mut stream = TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_secs(5))?;
        stream.set_read_timeout(Some(Duration::from_secs(60)))?;
        let body = body.map(Value::to_string).unwrap_or_default();
        let mut request = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
        if !body.is_empty() {
            request.push_str("Content-Type: application/json\r\n");
        }
        for (name, value) in headers {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        request.push_str(&body);
        stream.write_all(request.as_bytes())?;
        Ok(stream)
    }

    /// The status and the decoded body of the HTTP/1.1 answer `raw`, as far as it was received;
    /// `None` before its head is complete.
    fn parse_answer(raw: &[u8]) -> Option<(u16, Vec<u8>)> {
        let end = raw.windows(4).position(|window| window == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
        let status = head.split_whitespace().nth(1)?.parse().ok()?;
        let mut body = &raw[end + 4..];
        if !head.contains("transfer-encoding: chunked") {
            return Some((status, body.to_vec()));
        }
        let mut data = Vec::new();
        while let Some(line) = body.windows(2).position(|window| window == b"\r\n") {
            let size = String::from_utf8_lossy(&body[..line]).split(';').next().and_then(|size| usize::from_str_radix(size.trim(), 16).ok());
            let Some(size) = size.filter(|size| *size > 0) else { break };
            let end = (line + 2 + size).min(body.len());
            data.extend_from_slice(&body[line + 2..end]);
            body = &body[(end + 2).min(body.len())..];
        }
        Some((status, data))
    }

    /// `send`, with the answer read to its end: its status and decoded body.
    fn exchange(port: u16, method: &str, path: &str, headers: &[(&str, String)], body: Option<&Value>) -> std::io::Result<(u16, Vec<u8>)> {
        let mut stream = send(port, method, path, headers, body)?;
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw)?;
        parse_answer(&raw).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "incomplete HTTP answer"))
    }

    fn chat_body(content: &str, stream: bool) -> Value {
        json!({"model": CHAT_MODEL, "messages": [{"role": "user", "content": content}], "max_tokens": 4, "stream": stream})
    }

    fn keyed(bearer: &str, key: &str) -> [(&'static str, String); 2] {
        [("Authorization", format!("Bearer {bearer}")), ("Idempotency-Key", key.to_owned())]
    }

    /// Whether `answer` is a 429 `model_saturated`, which the gateway gives before reservation
    /// while a readiness probe holds the model's slots: the request may be sent again.
    fn saturated(answer: &(u16, Vec<u8>)) -> bool {
        answer.0 == 429 && contains(&answer.1, b"model_saturated")
    }

    /// One inference request, `POST path` with `body` under `key` with `bearer`, sent again while
    /// it is `saturated`: its status and decoded body.
    fn infer(port: u16, path: &str, bearer: &str, key: &str, body: &Value) -> (u16, Vec<u8>) {
        for _ in 0..100 {
            let answer = exchange(port, "POST", path, &keyed(bearer, key), Some(body)).unwrap_or_else(|error| panic!("{key}: {error}"));
            if !saturated(&answer) {
                return answer;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("{key}: the model stayed saturated");
    }

    /// One chat request under `key` with `bearer`: its status and decoded body.
    fn chat(port: u16, bearer: &str, key: &str, content: &str, stream: bool) -> (u16, Vec<u8>) {
        infer(port, "/v1/chat/completions", bearer, key, &chat_body(content, stream))
    }

    /// A streamed chat request whose answer has begun.
    struct OpenStream {
        stream: TcpStream,
        raw: Vec<u8>,
    }

    impl OpenStream {
        /// Sends a streamed chat request under `key` and reads its answer until its first token.
        fn open(port: u16, bearer: &str, key: &str, content: &str) -> Self {
            for _ in 0..100 {
                let mut stream = send(port, "POST", "/v1/chat/completions", &keyed(bearer, key), Some(&chat_body(content, true)))
                    .unwrap_or_else(|error| panic!("{key}: {error}"));
                let mut raw = Vec::new();
                let mut buffer = [0u8; 4096];
                loop {
                    if parse_answer(&raw).is_some_and(|(status, body)| status == 200 && contains(&body, b"\"OK")) {
                        return Self { stream, raw };
                    }
                    match stream.read(&mut buffer) {
                        Ok(read) if read > 0 => raw.extend_from_slice(&buffer[..read]),
                        _ => break,
                    }
                }
                let answer = parse_answer(&raw);
                assert!(answer.as_ref().is_some_and(saturated), "{key}: the stream ended before its first token: {}",
                    String::from_utf8_lossy(&raw));
                std::thread::sleep(Duration::from_millis(50));
            }
            panic!("{key}: the model stayed saturated");
        }
        /// Whether the daemon still holds the answer's connection open: no end of it arrives
        /// within 200 ms (at most two seconds while bytes keep arriving, which are kept).
        fn still_open(&mut self) -> bool {
            self.stream.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut buffer = [0u8; 4096];
            let open = loop {
                match self.stream.read(&mut buffer) {
                    Ok(0) => break false,
                    Ok(read) => self.raw.extend_from_slice(&buffer[..read]),
                    Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break true,
                    Err(_) => break false,
                }
                if Instant::now() >= deadline {
                    break true;
                }
            };
            self.stream.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
            open
        }
        /// Reads the rest of the answer until the daemon closes the connection: its status and
        /// decoded body.
        fn finish(mut self) -> (u16, Vec<u8>) {
            let _ = self.stream.read_to_end(&mut self.raw);
            parse_answer(&self.raw).unwrap()
        }
    }

    /// `GET path` on `provider`'s administration listener with the operator bearer: the answer.
    fn admin_answer(provider: &Provider, path: &str) -> Vec<u8> {
        let (status, body) = exchange(provider.admin, "GET", path, &[("Authorization", format!("Bearer {ADMIN_TOKEN}"))], None)
            .unwrap_or_else(|error| panic!("{path}: {error}"));
        assert_eq!(status, 200, "{path}");
        body
    }

    fn admin_get(provider: &Provider, path: &str) -> Value {
        serde_json::from_slice(&admin_answer(provider, path)).unwrap()
    }

    /// The request that `/admin/requests` lists under `key`.
    fn listed(provider: &Provider, key: &str) -> Option<Value> {
        admin_get(provider, "/admin/requests")["requests"].as_array().unwrap().iter().find(|request| request["request_id"] == key).cloned()
    }

    /// Sends `SIGTERM` to the daemon of `run` and waits, at most 90 seconds, until it exits.
    fn terminate(provider: &Provider, run: &mut Cleanup<'_>, case: &str) -> ExitStatus {
        assert_eq!(unsafe { libc::kill(run.daemon.id() as libc::pid_t, libc::SIGTERM) }, 0);
        wait(&mut run.daemon, Duration::from_secs(90)).unwrap_or_else(|| panic!("{case}: the daemon did not exit\n{}", logs(provider)))
    }

    /// Whether the process `pid` is gone within five seconds.
    fn gone(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while exists(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        !exists(pid)
    }

    /// Every file under `root`, recursively, in order.
    fn files_under(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![root.to_owned()];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() { pending.push(path); } else { found.push(path); }
            }
        }
        found.sort();
        found
    }

    /// Task 3.7 (requirement 7, scenario "A rejected chain identity stops the start"), at process
    /// level. A daemon with a managed stub engine whose chain RPC reports another chain ID, or
    /// another genesis, exits nonzero with "configured RPC identity was rejected", without its
    /// start line, without starting its engine and without printing a credential. The RPC holds
    /// its first request, the identity check, until the test has sent `GET /healthz` to the public
    /// listener, which the daemon binds before it starts: no listener ever answers that request.
    /// The control: with the configured identity the same daemon starts its engine and serves, and
    /// the held request is answered 200 once it serves; `SIGTERM` then ends it with status 0.
    pub(super) fn a_rejected_chain_identity_stops_the_daemon_before_it_serves_or_starts_an_engine(runtime: &Runtime) {
        let services = Services::new();
        let cases = [("another chain ID", "0x48", ESPACE_TESTNET_GENESIS.to_owned()),
            ("another genesis", "0x47", format!("0x{}", "99".repeat(32))),
            ("the configured identity", "0x47", ESPACE_TESTNET_GENESIS.to_owned())];
        for (index, (case, chain_id, genesis)) in cases.into_iter().enumerate() {
            let provider = runtime.provider(&format!("chain-identity-{index}"), "engine.token", None);
            std::fs::write(provider.directory.join(SERVES_HEALTH), b"").unwrap();
            let gate = Gate::new(true);
            chain_at(&provider, services.serve(chain_stub(chain_id, genesis, gate.clone())));
            let mut run = spawn(&provider);
            assert!(services.reached(&gate.entered, |entered| *entered),
                "{case}: the daemon did not check the RPC identity\n{}", logs(&provider));
            let mut pending = send(provider.public, "GET", "/healthz", &[], None).unwrap();
            gate.release();
            let mut answer = Vec::new();
            if case != "the configured identity" {
                let status = wait(&mut run.daemon, Duration::from_secs(60))
                    .unwrap_or_else(|| panic!("{case}: the daemon kept starting\n{}", logs(&provider)));
                let report = logs(&provider);
                assert!(!status.success(), "{case}: {status:?}\n{report}");
                assert!(report.contains("configured RPC identity was rejected"), "{case}: {report}");
                assert!(!report.contains("Chiral provider public="), "{case}: the daemon served\n{report}");
                assert_no_secret(case, &report);
                assert!(provider.engine_starts().is_empty(), "{case}: an engine started\n{report}");
                let _ = pending.read_to_end(&mut answer);
                assert!(answer.is_empty(), "{case}: a listener answered {}", String::from_utf8_lossy(&answer));
            } else {
                served(&provider, &mut run, case);
                pending.read_to_end(&mut answer).unwrap();
                assert!(answer.starts_with(b"HTTP/1.1 200"), "{case}: {}", String::from_utf8_lossy(&answer));
                assert_eq!(provider.engine_starts().len(), 1, "{case}\n{}", logs(&provider));
                let helper = provider.helper().unwrap();
                assert_eq!(terminate(&provider, &mut run, case).code(), Some(0), "{case}\n{}", logs(&provider));
                assert!(gone(helper), "{case}: the engine outlived the daemon");
            }
        }
    }

    /// Task 3.8 (requirement 8, scenario "A second daemon is refused before it joins the network"),
    /// at process level. While a daemon serves its deployment with a managed stub engine, a second
    /// daemon process for the same deployment, configured in the same directory (so with the same
    /// data directory, signing key and DHT identity file) but with other public, administration,
    /// engine and DHT listen ports and a bootstrap peer of its own (a plain listener that counts the
    /// connections it accepts), exits nonzero with the deployment lock's error, without its start
    /// line and without printing a credential: the stub engine's start log shows no second engine,
    /// and the bootstrap listener has accepted no connection. The first daemon still serves, and
    /// `SIGTERM` ends it with status 0. The control: the second configuration then starts, under
    /// the same peer identity, starts its engine and dials its bootstrap peer.
    pub(super) fn a_second_daemon_for_a_running_deployment_is_refused_before_it_joins_or_starts_an_engine(runtime: &Runtime) {
        let provider = runtime.provider("second-daemon", "engine.token", None);
        std::fs::write(provider.directory.join(SERVES_HEALTH), b"").unwrap();
        let mut first = spawn(&provider);
        let peer = served(&provider, &mut first, "first");
        assert_eq!(provider.engine_starts().len(), 1, "{}", logs(&provider));
        let bootstrap = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        bootstrap.set_nonblocking(true).unwrap();
        let mut held = Vec::new();
        // The connections accepted so far, held open and never answered.
        let mut accepted = || {
            while let Ok((stream, _)) = bootstrap.accept() {
                held.push(stream);
            }
            held.len()
        };
        let bootstrap_peer = libp2p::identity::Keypair::generate_ed25519().public().to_peer_id();
        let second = provider.directory.join("second.json");
        let mut config: Value = serde_json::from_slice(&std::fs::read(&provider.config).unwrap()).unwrap();
        let [public, admin, engine] = [(); 3].map(|_| free_port());
        config["public_bind"] = json!(format!("127.0.0.1:{public}"));
        config["public_endpoint"] = json!(format!("http://127.0.0.1:{public}"));
        config["admin_bind"] = json!(format!("127.0.0.1:{admin}"));
        config["models"][0]["engine_url"] = json!(format!("http://127.0.0.1:{engine}"));
        config["dht"]["bootstrap"] = json!([format!("/ip4/127.0.0.1/tcp/{}/p2p/{bootstrap_peer}",
            bootstrap.local_addr().unwrap().port())]);
        std::fs::write(&second, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let command = || {
            let mut command = Command::new(env!("CARGO_BIN_EXE_chiral_daemon"));
            command.env_remove("CHIRAL_PROVIDER_CONFIG").arg("--config").arg(&second);
            command
        };

        let mut refused = spawn_named(&provider, command(), "second");
        let status = wait(&mut refused.daemon, Duration::from_secs(60))
            .unwrap_or_else(|| panic!("the second daemon kept running\n{}", logs_named(&provider, "second")));
        let report = logs_named(&provider, "second");
        assert!(!status.success(), "{status:?}\n{report}");
        assert!(report.contains("another provider owns this deployment data directory"), "{report}");
        assert!(!report.contains("Chiral provider public="), "the second daemon served\n{report}");
        assert_no_secret("second daemon", &report);
        assert_eq!(provider.engine_starts().len(), 1, "the second daemon started an engine\n{report}");
        assert_eq!(accepted(), 0, "the second daemon connected to its bootstrap peer under the deployment's identity");
        assert!(healthz(provider.public), "the first daemon stopped serving\n{}", logs(&provider));
        let helper = provider.helper().unwrap();
        assert_eq!(terminate(&provider, &mut first, "first").code(), Some(0), "{}", logs(&provider));
        assert!(gone(helper), "the first daemon's engine outlived it");

        // The control: once the first daemon has exited, the second configuration owns the deployment.
        let mut second_run = spawn_named(&provider, command(), "second");
        assert_eq!(served_named(&provider, &mut second_run, "the second configuration", "second"), peer);
        let deadline = Instant::now() + Duration::from_secs(30);
        while accepted() == 0 {
            assert!(Instant::now() < deadline, "the second configuration did not dial its bootstrap peer");
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(provider.engine_starts().len(), 2, "{}", logs_named(&provider, "second"));
        let helper = provider.helper().unwrap();
        assert_eq!(unsafe { libc::kill(second_run.daemon.id() as libc::pid_t, libc::SIGTERM) }, 0);
        let status = wait(&mut second_run.daemon, Duration::from_secs(90));
        assert_eq!(status.and_then(|status| status.code()), Some(0), "{}", logs_named(&provider, "second"));
        assert!(gone(helper), "the second configuration's engine outlived it");
    }

    /// Task 3.9 (requirement 9, scenario "Legacy data is left in place"). The base data directory
    /// also holds legacy wallet, chain, Drive, hosting and file-transfer directories, the wallet
    /// one with balances. A daemon
    /// serving the chat model of the operator-owned stub engine starts, serves an account's request
    /// and its administration routes, and shuts down on `SIGTERM` with status 0. Every legacy file
    /// is unchanged, no file is added to or removed from those directories, and the only account
    /// is the existing free one, without funding, credit or spending.
    pub(super) fn legacy_wallet_chain_and_drive_data_stays_in_place_through_start_serve_and_shutdown(runtime: &Runtime) {
        let services = Services::new();
        let engine = services.serve(engine_stub(1, Held::new()));
        let provider = runtime.provider("legacy-data", "engine.token", None);
        unmanaged(&provider, engine, 1);
        let data = provider.directory.join("data");
        let legacy: [(&str, &[u8]); 8] = [
            ("wallet/keystore.json", br#"{"address":"0x5555555555555555555555555555555555555555","balance_wei":"123456789000000000000","crypto":{}}"#),
            ("wallet/accounts.json", br#"[{"name":"legacy","balance":"42 CHR"}]"#),
            ("geth/nodekey", b"legacy chain node key"),
            ("geth/chaindata/000001.log", b"legacy chain data"),
            ("drive/manifest.json", br#"{"files":[{"name":"report.pdf","size":4}]}"#),
            ("drive/files/report.pdf", b"%PDF"),
            ("hosting/sites.json", br#"[{"site":"legacy","files":1}]"#),
            ("downloads/transfer.part", b"legacy file-transfer data"),
        ];
        for (path, contents) in legacy {
            let path = data.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        }
        let digests = || -> Vec<(PathBuf, String)> {
            ["wallet", "geth", "drive", "hosting", "downloads"].iter().flat_map(|directory| files_under(&data.join(directory)))
                .map(|path| (path.clone(), hex::encode(Sha256::digest(std::fs::read(&path).unwrap())))).collect()
        };
        let before = digests();
        assert_eq!(before.len(), legacy.len());
        let (contract, bearer) = seed_free_account(&provider, "22", 1);
        let mut run = spawn(&provider);
        served(&provider, &mut run, "legacy");
        assert_eq!(chat(provider.public, &bearer, "served-beside-legacy-data", "hello", false).0, 200, "{}", logs(&provider));
        for path in ["/admin/status", "/admin/runtime", "/admin/offers", "/admin/requests", "/admin/fees"] {
            admin_answer(&provider, path);
        }
        let accounts = admin_get(&provider, "/admin/accounts")["accounts"].as_array().unwrap().clone();
        assert_eq!(accounts.len(), 1);
        assert_eq!((&accounts[0]["contract_id"], &accounts[0]["status"]), (&json!(contract), &json!("free")));
        for member in ["funded_wei", "fee_wei", "spent_wei", "reserved_wei", "available_wei", "reversed_wei"] {
            assert_eq!(accounts[0][member], "0", "{member}");
        }
        assert_eq!(terminate(&provider, &mut run, "legacy").code(), Some(0), "{}", logs(&provider));
        assert_eq!(digests(), before, "a legacy file changed");
    }

    /// Task 3.10 (requirement 10, scenario "Editing the configuration of a running daemon changes
    /// nothing"), at process level. A daemon serving the chat model of the operator-owned stub
    /// engine, with a ready chain RPC, publishes its offer and serves a free account of that model.
    /// Its configuration file is edited to another price, another model revision, another minimum
    /// funding and another admission setting (two requests at a time instead of one), and `POST
    /// /admin/offers/refresh` runs a pass: the daemon keeps publishing every term it started with
    /// (under a higher revision) and keeps serving the account. After `SIGTERM` and a restart, its
    /// offer advertises the edited terms under a still higher revision, and the account, bound to
    /// the replaced revision, is refused before reservation.
    pub(super) fn editing_the_configuration_of_a_running_daemon_changes_nothing_until_it_restarts(runtime: &Runtime) {
        let services = Services::new();
        // The engine has as many slots as the daemon's configuration names; the operator gives it
        // a second slot with the edit, before the restart.
        let slots = Arc::new(std::sync::atomic::AtomicU32::new(1));
        let engine = services.serve(engine_stub_of(slots.clone(), Held::new()));
        let provider = runtime.provider("edited-configuration", "engine.token", None);
        unmanaged(&provider, engine, 1);
        chain_at(&provider, services.serve(chain_stub("0x47", ESPACE_TESTNET_GENESIS.into(), Gate::new(false))));
        let (_, bearer) = seed_free_account(&provider, "22", 1);
        let offer = |provider: &Provider| {
            let offers = admin_get(provider, "/admin/offers")["offers"].as_array().unwrap().clone();
            assert_eq!(offers.len(), 1, "{}", logs(provider));
            offers[0]["payload"].clone()
        };
        let mut run = spawn(&provider);
        served(&provider, &mut run, "start");
        let first = offer(&provider);
        assert_eq!((&first["model"], &first["price"], &first["min_funding_wei"], &first["max_concurrency"]),
            (&chat_model(), &json!({"input_wei_per_1k": "1000", "output_wei_per_1k": "2000"}), &json!("1000"), &json!(1)));
        assert_eq!(chat(provider.public, &bearer, "before-the-edit", "hello", false).0, 200);
        // Every term of an offer but its revision and its validity times.
        let terms = |offer: &Value| {
            let mut terms = offer.clone();
            for field in ["revision", "issued_at", "expires_at"] {
                terms.as_object_mut().unwrap().remove(field);
            }
            terms
        };

        edit_json(&provider.config, |config| {
            config["models"][0]["price"] = json!({"input_wei_per_1k": "3000", "output_wei_per_1k": "4000"});
            config["models"][0]["model"]["revision"] = json!("cd".repeat(20));
            config["models"][0]["max_concurrency"] = json!(2);
            config["min_funding_wei"] = json!("5000");
        });
        let (status, body) = exchange(provider.admin, "POST", "/admin/offers/refresh",
            &[("Authorization", format!("Bearer {ADMIN_TOKEN}"))], None).unwrap();
        assert_eq!((status, serde_json::from_slice::<Value>(&body).unwrap()), (200, json!({"accepting": true, "offers_published": 1})));
        let second = offer(&provider);
        assert_eq!(terms(&second), terms(&first));
        assert!(second["revision"].as_u64() > first["revision"].as_u64(), "{} {}", first["revision"], second["revision"]);
        assert_eq!(chat(provider.public, &bearer, "after-the-edit", "hello", false).0, 200);
        assert_eq!(terminate(&provider, &mut run, "edited").code(), Some(0), "{}", logs(&provider));
        slots.store(2, Ordering::Release);

        let mut run = spawn(&provider);
        served(&provider, &mut run, "restart");
        let third = offer(&provider);
        assert_eq!((&third["model"]["revision"], &third["price"], &third["min_funding_wei"], &third["max_concurrency"]),
            (&json!("cd".repeat(20)), &json!({"input_wei_per_1k": "3000", "output_wei_per_1k": "4000"}), &json!("5000"), &json!(2)));
        assert!(third["revision"].as_u64() > second["revision"].as_u64(), "{} {}", second["revision"], third["revision"]);
        let (status, body) = chat(provider.public, &bearer, "after-the-restart", "hello", false);
        assert_eq!(status, 409, "{}", String::from_utf8_lossy(&body));
        assert!(contains(&body, b"model_revision_unavailable"), "{}", String::from_utf8_lossy(&body));
        for (key, status) in [("before-the-edit", Some("completed")), ("after-the-edit", Some("completed")), ("after-the-restart", None)] {
            assert_eq!(listed(&provider, key).map(|request| request["status"].clone()), status.map(|status| json!(status)), "{key}");
        }
        assert_eq!(terminate(&provider, &mut run, "restarted").code(), Some(0), "{}", logs(&provider));
    }

    /// Task 3.11 (requirement 11, scenario "Termination lets an admitted request settle"). A daemon
    /// serving the chat model of the operator-owned stub engine (two slots) admits a free account's
    /// streamed request, which the engine holds open after its first token; while it is held, a
    /// second request of the account completes (the control). On `SIGTERM`, `/readyz` stops
    /// answering ready, a new request of the account is refused at once (no listener accepts it,
    /// or the drained gateway answers 503 `provider_paused`), and the daemon keeps running for
    /// the admitted stream: once its public listener has stopped accepting, it keeps running, and
    /// keeps the stream open, for three seconds, which a shutdown that cuts admitted streams
    /// inside its bound does not. Released, the stream completes (`[DONE]`) and the daemon exits with
    /// status 0. After a restart, the stream is recorded as completed with the engine's usage, and
    /// the refused request is not recorded.
    pub(super) fn sigterm_with_an_admitted_stream_refuses_new_requests_and_exits_once_it_settles(runtime: &Runtime) {
        let services = Services::new();
        let held = Held::new();
        let engine = services.serve(engine_stub(2, held.clone()));
        let provider = runtime.provider("sigterm-admitted", "engine.token", None);
        unmanaged(&provider, engine, 2);
        let (_, bearer) = seed_free_account(&provider, "22", 2);
        let mut run = spawn(&provider);
        served(&provider, &mut run, "start");
        let mut admitted = OpenStream::open(provider.public, &bearer, "admitted-before-sigterm", "held");
        assert!(services.reached(&held.entered, |count| *count >= 1), "the engine did not hold the stream");
        assert_eq!(chat(provider.public, &bearer, "control", "hello", false).0, 200);
        assert_eq!(exchange(provider.public, "GET", "/readyz", &[], None).unwrap().0, 200);

        assert_eq!(unsafe { libc::kill(run.daemon.id() as libc::pid_t, libc::SIGTERM) }, 0);
        let deadline = Instant::now() + Duration::from_secs(30);
        while exchange(provider.public, "GET", "/readyz", &[], None).is_ok_and(|(status, _)| status == 200) {
            assert!(Instant::now() < deadline, "the daemon did not drain on SIGTERM\n{}", logs(&provider));
            std::thread::sleep(Duration::from_millis(20));
        }
        match exchange(provider.public, "POST", "/v1/chat/completions", &keyed(&bearer, "after-sigterm"), Some(&chat_body("hello", false))) {
            Err(_) => {}
            Ok((status, body)) => assert!(status == 503 && contains(&body, b"provider_paused"),
                "a new request was not refused: {status} {}", String::from_utf8_lossy(&body)),
        }
        assert!(run.daemon.try_wait().unwrap().is_none(), "the daemon exited before the admitted stream settled\n{}", logs(&provider));
        // The public listener stops accepting once the shutdown drain has returned (step 3), and
        // the daemon then waits, within its bound, for the admitted stream. A shutdown that cut
        // the stream inside its bound (a bound of 0, or server tasks aborted before the wait)
        // exits within a fraction of a second of that point, so the stream would otherwise race
        // its release: for three seconds after the listener closed, the daemon keeps running and
        // keeps the stream open. No signal marks a daemon that waits correctly, so this is a
        // fixed window for an absence, not for ordering.
        let deadline = Instant::now() + Duration::from_secs(30);
        while TcpStream::connect(("127.0.0.1", provider.public)).is_ok() {
            assert!(Instant::now() < deadline, "the public listener kept accepting after SIGTERM\n{}", logs(&provider));
            std::thread::sleep(Duration::from_millis(20));
        }
        let closed = Instant::now();
        while closed.elapsed() < Duration::from_secs(3) {
            assert!(run.daemon.try_wait().unwrap().is_none(),
                "the daemon exited inside its bound while a stream was admitted\n{}", logs(&provider));
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(admitted.still_open(), "the daemon ended the admitted stream inside its bound");
        held.release();
        let (status, body) = admitted.finish();
        assert_eq!(status, 200);
        assert!(contains(&body, b"[DONE]"), "the admitted stream did not complete: {}", String::from_utf8_lossy(&body));
        let status = wait(&mut run.daemon, Duration::from_secs(90)).unwrap_or_else(|| panic!("the daemon did not exit\n{}", logs(&provider)));
        assert_eq!(status.code(), Some(0), "{}", logs(&provider));

        let mut run = spawn(&provider);
        served(&provider, &mut run, "restart");
        let settled = listed(&provider, "admitted-before-sigterm").expect("the admitted request is not recorded");
        assert_eq!((&settled["status"], &settled["usage"]["prompt_tokens"], &settled["usage"]["completion_tokens"]),
            (&json!("completed"), &json!(2), &json!(1)));
        assert_eq!(listed(&provider, "control").map(|request| request["status"].clone()), Some(json!("completed")));
        assert!(listed(&provider, "after-sigterm").is_none(), "the refused request was recorded");
        assert_eq!(terminate(&provider, &mut run, "restarted").code(), Some(0), "{}", logs(&provider));
    }

    /// Task 3.11 (requirement 11, scenario "A request that outlasts the bound is released, not
    /// charged"). Without a test-only bound in the daemon, the test waits the daemon's own 75-second
    /// shutdown bound. A daemon serving the chat model of the operator-owned stub engine (one slot)
    /// admits a free account's streamed request (one request at a time), which the engine holds
    /// open after its first token and never completes. On `SIGTERM`, the daemon waits for it for
    /// its whole bound and then exits by itself; the stream never completes. After a restart, the
    /// request is released under the unknown-usage rule (`interrupted_unknown_usage` at shutdown, or
    /// `restart_unknown_usage` by the start's recovery), without usage or charge, none is unsettled,
    /// and the account's next request is admitted, so its hold is released.
    pub(super) fn a_stream_that_outlasts_the_shutdown_bound_is_released_under_unknown_usage(runtime: &Runtime) {
        let services = Services::new();
        let held = Held::new();
        let engine = services.serve(engine_stub(1, held.clone()));
        let provider = runtime.provider("outlasts-the-bound", "engine.token", None);
        unmanaged(&provider, engine, 1);
        let (_, bearer) = seed_free_account(&provider, "22", 1);
        let mut run = spawn(&provider);
        served(&provider, &mut run, "start");
        let admitted = OpenStream::open(provider.public, &bearer, "outlasts-the-bound", "held");
        assert!(services.reached(&held.entered, |count| *count >= 1), "the engine did not hold the stream");
        let signalled = Instant::now();
        assert_eq!(unsafe { libc::kill(run.daemon.id() as libc::pid_t, libc::SIGTERM) }, 0);
        let status = wait(&mut run.daemon, SHUTDOWN_BOUND + Duration::from_secs(45))
            .unwrap_or_else(|| panic!("the daemon did not exit after its bound\n{}", logs(&provider)));
        let waited = signalled.elapsed();
        assert!(status.code().is_some(), "the daemon did not exit by itself: {status:?}\n{}", logs(&provider));
        assert!(waited >= SHUTDOWN_BOUND, "the daemon ended the admitted stream after {waited:?}, before its bound\n{}", logs(&provider));
        let (_, body) = admitted.finish();
        assert!(!contains(&body, b"[DONE]"), "the held stream completed");
        held.release();

        let mut run = spawn(&provider);
        served(&provider, &mut run, "restart");
        let released = listed(&provider, "outlasts-the-bound").expect("the admitted request is not recorded");
        assert!(matches!(released["status"].as_str(), Some("interrupted_unknown_usage" | "restart_unknown_usage")),
            "{}", released["status"]);
        assert_eq!((&released["usage"]["prompt_tokens"], &released["usage"]["completion_tokens"], &released["cost_wei"]),
            (&json!(0), &json!(0), &json!("0")));
        assert_eq!(admin_get(&provider, "/admin/status")["ledger"]["active_requests"], 0);
        assert_eq!(chat(provider.public, &bearer, "after-the-restart", "hello", false).0, 200, "the account's hold was not released");
        assert_eq!(terminate(&provider, &mut run, "restarted").code(), Some(0), "{}", logs(&provider));
        eprintln!("the request that outlasted the bound: {} after {waited:?}; the daemon exited with {status:?}", released["status"]);
    }

    /// Fails, naming only `context` and what it carries, when `bytes` carry either marker, the
    /// account `bearer` or another credential of the provider.
    /// The digits that every printing of `VECTOR_MARKER`, as a 64-bit or a 32-bit float, begins with.
    const VECTOR_DIGITS: &str = "0.61803";

    /// Fails, naming only `context` and what it carries, when `bytes` carry a marker of request
    /// content, an account bearer of `bearers` or another credential of the provider.
    fn assert_carries_nothing_private(context: &str, bytes: &[u8], bearers: &[&str]) {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains(PROMPT_MARKER), "{context}: carries a prompt or embedding input");
        assert!(!text.contains(COMPLETION_MARKER), "{context}: carries a completion");
        assert!(!text.contains(TOOL_MARKER), "{context}: carries tool arguments");
        assert!(!text.contains(VECTOR_DIGITS), "{context}: carries an embedding vector");
        assert!(!bearers.iter().any(|bearer| text.contains(bearer)), "{context}: carries an account bearer");
        assert_no_secret(context, &text);
    }

    /// Task 3.12 (requirement 12, scenario "Neither content nor secrets reach logs or
    /// administration"). A daemon serves the chat model of the operator-owned stub engine, with
    /// tools, and an embedding model of a second stub engine. With account bearers it receives
    /// requests whose prompts carry a marker: a chat request completed unstreamed and one completed
    /// streamed (whose completions carry a second marker), a tool request unstreamed and one
    /// streamed (whose generated tool arguments carry a third), an embedding request (whose input
    /// carries the marker and whose vectors are `VECTOR_MARKER`), a chat request that fails at the
    /// engine (whose error repeats the prompt), and one that its owner cancels while the engine
    /// holds it. Each answer reaches its client with its content. While the engine holds that
    /// last request, reserved, no file under the data directory (the ledger's WAL included)
    /// carries a marker or a vector, and no `GET /admin/*` answer carries what the next sentence
    /// excludes. Once that request is cancelled, every `GET /admin/*` answer carries no marker, no
    /// vector, no account bearer, and not the administration token, the engine key, the RPC bearer
    /// or the signing key; `/admin/requests` lists each request with its
    /// identifier, status, usage, cost and timing. After `SIGTERM` (status 0), the daemon's
    /// standard output and error carry none of them either, and no file under the data directory
    /// (the deployment's ledger with its WAL and lock, the wallet journal and the policy directory)
    /// carries a marker or a vector.
    pub(super) fn prompts_completions_and_credentials_reach_no_output_administration_answer_or_deployment_file(runtime: &Runtime) {
        let services = Services::new();
        let held = Held::new();
        let engine = services.serve(engine_stub(1, held.clone()));
        let embedder = services.serve(embedding_stub());
        let provider = runtime.provider("no-content", "engine.token", None);
        unmanaged(&provider, engine, 1);
        edit_json(&provider.config, |config| {
            config["models"][0]["model"]["capabilities"] = json!(["chat", "tools"]);
            let mut embedding = config["models"][0].clone();
            embedding["model"] = embedding_model();
            embedding["engine_url"] = json!(format!("http://127.0.0.1:{embedder}"));
            embedding["price"] = json!({"input_wei_per_1k": "1000", "output_wei_per_1k": "0"});
            config["models"].as_array_mut().unwrap().push(embedding);
        });
        let (_, bearer) = seed_free_account(&provider, "22", 1);
        let (_, embedding_bearer) = seed_free_account_of(&provider, "33", 1, 1);
        let bearers = [bearer.as_str(), embedding_bearer.as_str()];
        let mut run = spawn(&provider);
        served(&provider, &mut run, "start");
        let prompt = |words: &str| format!("{words} {PROMPT_MARKER}");
        let (status, body) = chat(provider.public, &bearer, "completed-unstreamed", &prompt("say"), false);
        assert_eq!(status, 200, "the unstreamed request failed");
        assert!(contains(&body, COMPLETION_MARKER.as_bytes()), "the unstreamed completion did not reach its client");
        let (status, body) = chat(provider.public, &bearer, "completed-streamed", &prompt("say"), true);
        assert_eq!(status, 200, "the streamed request failed");
        assert!(contains(&body, COMPLETION_MARKER.as_bytes()) && contains(&body, b"[DONE]"), "the streamed completion did not reach its client");
        let tools = json!([{"type": "function", "function": {"name": "ping", "parameters": {"type": "object", "properties": {}}}}]);
        for (key, stream) in [("tool-call-unstreamed", false), ("tool-call-streamed", true)] {
            let mut body = chat_body(&prompt("call"), stream);
            body["tools"] = tools.clone();
            let (status, answer) = infer(provider.public, "/v1/chat/completions", &bearer, key, &body);
            assert_eq!(status, 200, "{key} failed");
            assert!(contains(&answer, TOOL_MARKER.as_bytes()), "{key}: the tool arguments did not reach the client");
        }
        let (status, answer) = infer(provider.public, "/v1/embeddings", &embedding_bearer, "embedding",
            &json!({"model": EMBEDDING_MODEL, "input": prompt("embed")}));
        assert_eq!(status, 200, "the embedding request failed");
        assert!(contains(&answer, VECTOR_DIGITS.as_bytes()), "the embedding did not reach the client");
        let (status, _) = chat(provider.public, &bearer, "failed-at-the-engine", &prompt("fail"), false);
        assert!(status >= 500, "the engine failure answered {status}");
        let cancelled = OpenStream::open(provider.public, &bearer, "cancelled-by-its-owner", &prompt("held"));
        assert!(services.reached(&held.entered, |count| *count >= 1), "the engine did not hold the stream");
        // While the engine holds it, the request is reserved. What the daemon keeps meanwhile (the
        // ledger's WAL before its checkpoint, or any file kept only while a request runs), and
        // what administration reports, carry no content either: content kept only until the
        // request settles, or until the ledger is checkpointed, is gone by the daemon's exit.
        assert_eq!(listed(&provider, "cancelled-by-its-owner").map(|request| request["status"].clone()), Some(json!("reserved")));
        assert_no_content_in_files(&provider.directory.join("data"), "while a request is reserved");
        for path in ["/admin/status", "/admin/runtime", "/admin/offers", "/admin/accounts", "/admin/requests", "/admin/fees"] {
            assert_carries_nothing_private(&format!("{path} while a request is reserved"), &admin_answer(&provider, path), &bearers);
        }
        let (status, _) = exchange(provider.public, "POST", "/v1/requests/cancelled-by-its-owner/cancel",
            &[("Authorization", format!("Bearer {bearer}"))], None).unwrap();
        assert_eq!(status, 202, "the cancellation was not accepted");
        let _ = cancelled.finish();
        let deadline = Instant::now() + Duration::from_secs(30);
        while listed(&provider, "cancelled-by-its-owner").is_none_or(|request| request["status"] == "reserved") {
            assert!(Instant::now() < deadline, "the cancelled request did not settle");
            std::thread::sleep(Duration::from_millis(20));
        }

        for path in ["/admin/status", "/admin/runtime", "/admin/offers", "/admin/accounts", "/admin/requests", "/admin/fees"] {
            assert_carries_nothing_private(path, &admin_answer(&provider, path), &bearers);
        }
        let requests = admin_get(&provider, "/admin/requests")["requests"].as_array().unwrap().clone();
        let expected = [("completed-unstreamed", "completed", 2, 1), ("completed-streamed", "completed", 2, 1),
            ("tool-call-unstreamed", "completed", 2, 1), ("tool-call-streamed", "completed", 2, 1), ("embedding", "completed", 2, 0),
            ("failed-at-the-engine", "failed_unknown_usage", 0, 0), ("cancelled-by-its-owner", "cancelled_unknown_usage", 0, 0)];
        assert_eq!(requests.len(), expected.len());
        for (key, status, prompt_tokens, completion_tokens) in expected {
            let request = requests.iter().find(|request| request["request_id"] == key).unwrap_or_else(|| panic!("{key} is not listed"));
            assert!(request["id"].as_str().is_some_and(|id| !id.is_empty()), "{key}: no identifier");
            assert_eq!((&request["status"], &request["usage"]["prompt_tokens"], &request["usage"]["completion_tokens"], &request["cost_wei"]),
                (&json!(status), &json!(prompt_tokens), &json!(completion_tokens), &json!("0")), "{key}");
            assert!(request["created_at"].is_u64() && request["settled_at"].is_u64() && request["performance"]["elapsed_us"].is_u64(),
                "{key}: no timing");
        }

        assert_eq!(terminate(&provider, &mut run, "no-content").code(), Some(0));
        assert_carries_nothing_private("the daemon's standard output and error", logs(&provider).as_bytes(), &bearers);
        let files = files_under(&provider.directory.join("data"));
        assert!(files.iter().any(|file| file.ends_with("ledger.sqlite3")) && files.iter().any(|file| file.ends_with("provider.lock")),
            "{files:?}");
        assert_no_content_in_files(&provider.directory.join("data"), "after the daemon exited");
    }

    /// Fails, naming only `context` and the file, when a file under `root` carries a marker of
    /// request content or the vector digits. A file removed between the listing and its reading
    /// is skipped.
    fn assert_no_content_in_files(root: &Path, context: &str) {
        for file in files_under(root) {
            let Ok(bytes) = std::fs::read(&file) else { continue };
            assert!([PROMPT_MARKER, COMPLETION_MARKER, TOOL_MARKER, VECTOR_DIGITS].iter().all(|marker| !contains(&bytes, marker.as_bytes())),
                "{context}: {file:?} carries request content");
        }
    }

    /// A chain RPC that answers as `chain_stub` does for the configured identity, and records the
    /// path and JSON-RPC method of every request it receives, on any path.
    #[cfg(target_os = "macos")]
    fn recording_chain_stub(seen: Arc<std::sync::Mutex<Vec<(String, String)>>>) -> axum::Router {
        use axum::{http::StatusCode, response::IntoResponse, Json};
        axum::Router::new().fallback(move |uri: axum::http::Uri, body: axum::body::Bytes| {
            let seen = seen.clone();
            async move {
                let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                seen.lock().unwrap().push((uri.path().to_owned(), request["method"].as_str().unwrap_or("<not JSON-RPC>").to_owned()));
                match chain_answer("0x47", ESPACE_TESTNET_GENESIS, &request) {
                    Some(answer) => Json(answer).into_response(),
                    None => StatusCode::SERVICE_UNAVAILABLE.into_response(),
                }
            }
        })
    }

    /// `provider`'s daemon, started suspended and watched by kqueue from before its first
    /// instruction (`process_observer`), with its output in `observed.stdout` and
    /// `observed.stderr`; neither the provider configuration variable nor a proxy variable is
    /// inherited.
    #[cfg(target_os = "macos")]
    fn observed_daemon(provider: &Provider) -> process_observer::Observed {
        let mut remove = vec!["CHIRAL_PROVIDER_CONFIG"];
        remove.extend(process_observer::PROXY_VARIABLES);
        let environment = process_observer::environment(&remove, &[]);
        process_observer::Observed::spawn(Path::new(env!("CARGO_BIN_EXE_chiral_daemon")),
            &[std::ffi::OsStr::new("--config"), provider.config.as_os_str()], &environment,
            &provider.directory.join("observed.stdout"), &provider.directory.join("observed.stderr"))
    }

    /// Waits until the observed daemon has printed its start line, then checks that `/healthz`
    /// answers 200.
    #[cfg(target_os = "macos")]
    fn observed_served(provider: &Provider, daemon: &mut process_observer::Observed, case: &str) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while !logs_named(provider, "observed").contains("Chiral provider public=") {
            assert!(daemon.try_wait().is_none(), "{case}: the daemon exited before serving\n{}", logs_named(provider, "observed"));
            assert!(Instant::now() < deadline, "{case}: the daemon did not serve\n{}", logs_named(provider, "observed"));
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(healthz(provider.public), "{case}: /healthz did not answer\n{}", logs_named(provider, "observed"));
    }

    /// Kills, when dropped, the stub engines that `provider`'s daemons started.
    #[cfg(target_os = "macos")]
    struct StubEngines<'a>(&'a Provider);

    #[cfg(target_os = "macos")]
    impl Drop for StubEngines<'_> {
        fn drop(&mut self) {
            kill_stub_engines(self.0);
        }
    }

    /// `application-startup-boundaries` task 3.1 (requirement 1, scenario "A provider starts no
    /// node or miner"), macOS part, for an external engine. A development daemon serving the chat
    /// model of an operator-owned engine (this test process's `engine_stub`), with its chain RPC
    /// at `/espace/rpc` of a recording loopback stub, starts, serves a free account's request
    /// and exits with status 0 on `SIGTERM`. Watched by kqueue from before its first instruction
    /// until it was reaped, it posted no fork notification: it created no process at all. Every
    /// chain request reached the configured path and was an `eth_*` method, the identity check
    /// first. Would catch a node, a miner or any other process that the daemon starts, and a
    /// chain request to another path or of another namespace.
    #[cfg(target_os = "macos")]
    pub(super) fn an_unmanaged_daemon_starts_no_process_and_reaches_only_its_configured_chain_rpc(runtime: &Runtime) {
        let services = Services::new();
        let engine = services.serve(engine_stub(1, Held::new()));
        let provider = runtime.provider("observed-unmanaged", "engine.token", None);
        unmanaged(&provider, engine, 1);
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let rpc = services.serve(recording_chain_stub(seen.clone()));
        edit_json(&provider.config, |config| config["network"]["rpc_url"] = json!(format!("http://127.0.0.1:{rpc}/espace/rpc")));
        let (_, bearer) = seed_free_account(&provider, "22", 1);
        let mut daemon = observed_daemon(&provider);
        observed_served(&provider, &mut daemon, "unmanaged");
        assert_eq!(chat(provider.public, &bearer, "observed-unmanaged", "hello", false).0, 200, "{}", logs_named(&provider, "observed"));
        daemon.signal(libc::SIGTERM);
        let status = daemon.wait_timeout(Duration::from_secs(90));
        assert_eq!(status, Some(0), "{}", logs_named(&provider, "observed"));
        assert_eq!(daemon.forks(), 0, "the daemon created a process\n{}", logs_named(&provider, "observed"));
        let requests = seen.lock().unwrap().clone();
        assert!(requests.len() >= 2, "{requests:?}");
        assert_eq!(requests[0], ("/espace/rpc".to_owned(), "eth_chainId".to_owned()));
        assert!(requests.iter().all(|(path, method)| path == "/espace/rpc" && method.starts_with("eth_")), "{requests:?}");
    }

    /// `application-startup-boundaries` task 3.1 (requirement 1, scenario "A provider starts no
    /// node or miner"), macOS part, for a managed runtime: an extension of the cases above. The
    /// development daemon with the CPU managed runtime, whose `llama-server` is the inventoried
    /// stub engine, and a recording chain RPC, is watched by kqueue from before its first
    /// instruction. Once it serves, its only child is the stub engine (the process that recorded
    /// itself as the one engine start, running the inventoried `llama-server`) and it has reaped
    /// no child, so it has created no other process while starting: the CPU profile runs no
    /// device observer. On `SIGTERM` it exits with status 0 without creating any further
    /// process (no new fork notification), and the engine no longer exists. Every chain request
    /// reached the configured path and was an `eth_*` method. Would catch a node, a miner or
    /// another process started beside the engine, during start, serving or shutdown.
    #[cfg(target_os = "macos")]
    pub(super) fn a_managed_daemon_starts_only_its_engine_through_start_serve_and_shutdown(runtime: &Runtime) {
        let services = Services::new();
        let provider = runtime.provider("observed-managed", "engine.token", None);
        std::fs::write(provider.directory.join(SERVES_HEALTH), b"").unwrap();
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let rpc = services.serve(recording_chain_stub(seen.clone()));
        edit_json(&provider.config, |config| config["network"]["rpc_url"] = json!(format!("http://127.0.0.1:{rpc}/espace/rpc")));
        let mut daemon = observed_daemon(&provider);
        let _engines = StubEngines(&provider);
        observed_served(&provider, &mut daemon, "managed");
        let report = || logs_named(&provider, "observed");
        let engine = provider.helper().unwrap_or_else(|| panic!("no engine\n{}", report()));
        assert_eq!(provider.engine_starts(), [engine.to_string()], "{}", report());
        assert_eq!(process_observer::children(daemon.pid()), [engine], "the daemon's children while serving");
        assert_eq!(process_observer::reaped_children_elapsed(daemon.pid()), 0, "the daemon reaped a child it had started while starting");
        assert_eq!(process_observer::executable(engine).canonicalize().unwrap(),
            runtime.root.path().join("runtime").join(ENGINE).canonicalize().unwrap());
        let forks = daemon.forks();
        assert!(forks >= 1, "the observation missed the engine's start");

        daemon.signal(libc::SIGTERM);
        assert!(daemon.exited_within(Duration::from_secs(90)), "the daemon did not exit\n{}", report());
        let engine_at_exit = exists(engine);
        assert_eq!(daemon.wait_timeout(Duration::from_secs(5)), Some(0), "{}", report());
        assert_eq!(daemon.forks(), forks, "the daemon created a process while serving or shutting down\n{}", report());
        assert!(!engine_at_exit, "the engine still existed when the daemon exited");
        let requests = seen.lock().unwrap().clone();
        assert!(!requests.is_empty() && requests.iter().all(|(path, method)| path == "/espace/rpc" && method.starts_with("eth_")), "{requests:?}");
    }
}
