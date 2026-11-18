//! Process tests of `chiral_daemon` with a development configuration whose CPU managed runtime's
//! `llama-server` is a full copy of this test binary, inventoried (`managed-engine-runtime`
//! requirements 6 and 11, tasks 2.1 and 2.3).
//!
//! This target has `harness = false` (Cargo.toml): libtest would reject the launcher's
//! `llama-server` arguments, so `main` acts as the stub engine when it runs under that name: it
//! records its process id next to its key file and sleeps without answering health. Under any
//! other name it runs the tests below in sequence; a name given on the command line filters them.
use chiral_network::{
    engine_runtime::{ENGINE_VERSION, SOURCE_REVISION},
    network::{ESPACE_TESTNET_CHAIN_ID, ESPACE_TESTNET_GENESIS},
    protocol, wallet,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::ExitStatus;
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const ENGINE: &str = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
const HELPER_PID: &str = "engine-helper.pid";
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
    ];
    #[cfg(unix)]
    tests.extend([
        ("sigterm_during_engine_startup_stops_the_engine", sigterm_during_engine_startup_stops_the_engine as fn(&Runtime)),
        ("sigint_during_engine_startup_stops_the_engine", sigint_during_engine_startup_stops_the_engine),
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

/// The stub engine: answers nothing, so the daemon waits for its health until the startup timeout.
fn engine_stub() {
    let arguments: Vec<_> = std::env::args_os().collect();
    let key = arguments.iter().position(|a| a == "--api-key-file").and_then(|i| arguments.get(i + 1));
    if let Some(directory) = key.map(Path::new).and_then(Path::parent) {
        let temporary = directory.join(format!("{HELPER_PID}.tmp"));
        if std::fs::write(&temporary, std::process::id().to_string()).is_ok() {
            let _ = std::fs::rename(&temporary, directory.join(HELPER_PID));
        }
    }
    // Bounded, so that a failed run cannot leave the stub behind for long.
    std::thread::sleep(Duration::from_secs(120));
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
        let template = b"{{ messages }}";
        let gguf = b"GGUF-test-inventory-bytes";
        std::fs::write(artifact.join("model-Q4_K_M.gguf"), gguf).unwrap();
        std::fs::write(artifact.join("chiral-chat-template.jinja"), template).unwrap();
        let model = json!({
            "id": "test/repo/model-Q4_K_M.gguf", "revision": "ab".repeat(20),
            "tokenizer_revision": "ab".repeat(20), "chat_template_sha256": hex::encode(Sha256::digest(template)),
            "quantization": "Q4_K_M", "context_length": 512, "max_output_tokens": 64,
            "capabilities": ["chat", "completions"], "serving": null
        });
        write_canonical(&artifact.join("chiral-model.json"), &model);
        write_canonical(&artifact.join("chiral-artifact-sha256.json"), &json!({
            "schema": "chiral-gguf-artifact-v1",
            "model": { "repo_id": "test/repo", "revision": model["revision"], "license": "test" },
            "task": "generation", "quantization": "Q4_K_M", "context_length": 512, "max_output_tokens": 64,
            "capabilities": model["capabilities"], "chat_template_name": "default", "model_file": "model-Q4_K_M.gguf",
            "files": {
                "model-Q4_K_M.gguf": { "sha256": hex::encode(Sha256::digest(gguf)), "size_bytes": gguf.len(),
                    "upstream_repo": "test/repo", "upstream_path": "model-Q4_K_M.gguf", "upstream_digest": "ab".repeat(32) },
                "chiral-chat-template.jinja": { "sha256": hex::encode(Sha256::digest(template)), "size_bytes": template.len(),
                    "upstream_repo": null, "upstream_path": null, "upstream_digest": null }
            }
        }));
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
        Provider { directory, config }
    }
}

struct Provider {
    directory: PathBuf,
    config: PathBuf,
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
    let log = |name: &str| std::fs::File::create(provider.directory.join(name)).unwrap();
    let daemon = provider.daemon(false).stdin(Stdio::null()).stdout(log("daemon.stdout")).stderr(log("daemon.stderr")).spawn().unwrap();
    Cleanup { daemon, provider }
}

fn logs(provider: &Provider) -> String {
    let read = |name: &str| std::fs::read_to_string(provider.directory.join(name)).unwrap_or_default();
    format!("stdout: {}\nstderr: {}", read("daemon.stdout"), read("daemon.stderr"))
}

#[cfg(unix)]
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
