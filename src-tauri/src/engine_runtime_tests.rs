use super::*;
use serde_json::json;
use std::io::Write;

fn fixture() -> (tempfile::TempDir, ModelConfig) {
    let directory = tempfile::tempdir().unwrap();
    let runtime = directory.path().join("runtime");
    let artifact = directory.path().join("artifact");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::create_dir(&artifact).unwrap();
    let key = directory.path().join("engine.key");
    crate::wallet::create_secret_file(&key).unwrap().write_all(b"test-private-engine-token-32-characters\n").unwrap();
    let mut executable_bytes = vec![0; 4096];
    let n = std::fs::File::open(std::env::current_exe().unwrap()).unwrap().read(&mut executable_bytes).unwrap();
    executable_bytes.truncate(n);
    let executable_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    std::fs::write(runtime.join(executable_name), &executable_bytes).unwrap();
    let runtime_manifest = json!({"schema":"chiral-llama-runtime-v1", "platform":platform(), "architecture":std::env::consts::ARCH,
        "backend":"cpu", "engine_version":ENGINE_VERSION, "source_revision":SOURCE_REVISION, "executable":executable_name,
        "files":{executable_name:{"sha256":hex::encode(Sha256::digest(&executable_bytes)),"size_bytes":executable_bytes.len()}}});
    std::fs::write(runtime.join("chiral-runtime.json"), protocol::canonical_json(&runtime_manifest).unwrap()).unwrap();
    let template = b"{{ messages }}";
    let gguf = b"GGUF-test-inventory-bytes";
    std::fs::write(artifact.join("model-Q4_K_M.gguf"), gguf).unwrap();
    std::fs::write(artifact.join("chiral-chat-template.jinja"), template).unwrap();
    let model = ModelSpec { id:"test/repo/model-Q4_K_M.gguf".into(), revision:"ab".repeat(20), tokenizer_revision:"ab".repeat(20), chat_template_sha256:hex::encode(Sha256::digest(template)), quantization:Some("Q4_K_M".into()), context_length:512, max_output_tokens:64, capabilities:vec!["chat".into(), "completions".into()], serving:None };
    std::fs::write(artifact.join("chiral-model.json"), protocol::canonical_json(&model).unwrap()).unwrap();
    let manifest = json!({"schema":"chiral-gguf-artifact-v1", "model":{"repo_id":"test/repo","revision":model.revision,"license":"test"},
        "task":"generation","quantization":"Q4_K_M","context_length":512,"max_output_tokens":64,"capabilities":model.capabilities,
        "chat_template_name":"default","model_file":"model-Q4_K_M.gguf",
        "files":{"model-Q4_K_M.gguf":{"sha256":hex::encode(Sha256::digest(gguf)),"size_bytes":gguf.len(),"upstream_repo":"test/repo","upstream_path":"model-Q4_K_M.gguf","upstream_digest":"ab".repeat(32)},
        "chiral-chat-template.jinja":{"sha256":hex::encode(Sha256::digest(template)),"size_bytes":template.len(),"upstream_repo":null,"upstream_path":null,"upstream_digest":null}}});
    std::fs::write(artifact.join("chiral-artifact-sha256.json"), protocol::canonical_json(&manifest).unwrap()).unwrap();
    let config = ModelConfig { model, price:crate::protocol::PriceSchedule { input_wei_per_1k:"0".into(), output_wei_per_1k:"0".into() }, engine_url:"http://127.0.0.1:18080".into(), engine_api_key_file:Some(key), max_concurrency:2, request_timeout_secs:30, qualification_evidence_file:None,
        runtime:Some(EngineRuntimeConfig { runtime_dir:runtime, artifact_dir:artifact, backend:"cpu".into(), device_ids:vec![], memory_reserve_mib:0, gpu_layers:0, split_mode:"none".into(), tensor_split:vec![], cache_type_k:"f16".into(), cache_type_v:"f16".into(), flash_attention:false, batch_size:128, ubatch_size:64, startup_timeout_secs:1 }) };
    (directory, config)
}

fn embedding_fixture() -> (tempfile::TempDir, ModelConfig) {
    let (directory, mut config) = fixture();
    let root = &config.runtime.as_ref().unwrap().artifact_dir;
    let path = root.join("chiral-artifact-sha256.json");
    let mut manifest = read_json(&path).unwrap();
    config.model.capabilities = vec!["embeddings".into()];
    config.model.max_output_tokens = 0;
    config.model.chat_template_sha256 = hex::encode(Sha256::digest(b""));
    manifest["task"] = json!("embeddings");
    manifest["capabilities"] = json!(config.model.capabilities);
    manifest["max_output_tokens"] = json!(0);
    manifest["chat_template_name"] = Value::Null;
    manifest["files"].as_object_mut().unwrap().remove("chiral-chat-template.jinja");
    std::fs::remove_file(root.join("chiral-chat-template.jinja")).unwrap();
    std::fs::write(root.join("chiral-model.json"), protocol::canonical_json(&config.model).unwrap()).unwrap();
    std::fs::write(path, protocol::canonical_json(&manifest).unwrap()).unwrap();
    (directory, config)
}

#[test]
fn embedding_inventory_rejects_selected_templates_even_when_hashes_match() {
    for source in [b"{{ messages }}".as_slice(), b"".as_slice()] {
        for selected in [Value::Null, json!("default")] {
            let (_directory, mut config) = embedding_fixture();
            let root = &config.runtime.as_ref().unwrap().artifact_dir;
            let path = root.join("chiral-artifact-sha256.json");
            let mut manifest = read_json(&path).unwrap();
            // Bind the exact selected bytes in both metadata and inventory; this
            // must not be rejected merely for a stale digest or missing file.
            config.model.chat_template_sha256 = hex::encode(Sha256::digest(source));
            manifest["chat_template_name"] = selected;
            manifest["files"]["chiral-chat-template.jinja"] = json!({
                "sha256": config.model.chat_template_sha256, "size_bytes": source.len(),
                "upstream_repo": null, "upstream_path": null, "upstream_digest": null
            });
            std::fs::write(root.join("chiral-chat-template.jinja"), source).unwrap();
            std::fs::write(root.join("chiral-model.json"), protocol::canonical_json(&config.model).unwrap()).unwrap();
            std::fs::write(path, protocol::canonical_json(&manifest).unwrap()).unwrap();
            assert!(validate_inventory(&config).is_err());
        }
    }
}

#[test]
fn embedding_inventory_requires_explicit_null_template_selection() {
    let (_directory, config) = embedding_fixture();
    let path = config.runtime.as_ref().unwrap().artifact_dir.join("chiral-artifact-sha256.json");
    let mut manifest = read_json(&path).unwrap();
    for selected in [
        Some(json!("default")), Some(json!("tool_use")), Some(json!("")),
        Some(json!(false)), Some(json!(0)), Some(json!([])), Some(json!({})), None,
    ] {
        if let Some(value) = selected {
            manifest["chat_template_name"] = value;
        } else {
            manifest.as_object_mut().unwrap().remove("chat_template_name");
        }
        std::fs::write(&path, protocol::canonical_json(&manifest).unwrap()).unwrap();
        assert!(validate_inventory(&config).is_err());
    }
    manifest["chat_template_name"] = Value::Null;
    std::fs::write(path, protocol::canonical_json(&manifest).unwrap()).unwrap();
    let validated = validate_inventory(&config).unwrap();
    assert_eq!(validated.task, "embeddings");
    assert!(validated.template.is_none());
}

#[test]
fn template_free_embedding_inventory_still_binds_model_bytes_and_empty_digest() {
    // Reuse synthetic inventory bytes, not an executable GGUF or engine qualification.
    let (_directory, mut config) = embedding_fixture();
    let root = &config.runtime.as_ref().unwrap().artifact_dir;
    let model_path = root.join("model-Q4_K_M.gguf");
    let validated = validate_inventory(&config).unwrap();
    assert_eq!(validated.task, "embeddings");
    assert!(validated.template.is_none());
    assert_eq!(validated.model_file, model_path.canonicalize().unwrap());

    let mut bytes = std::fs::read(&model_path).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&model_path, &bytes).unwrap();
    assert!(validate_inventory(&config).is_err());
    *bytes.last_mut().unwrap() ^= 1;
    std::fs::write(&model_path, &bytes).unwrap();

    config.model.chat_template_sha256 = hex::encode(Sha256::digest(b"{{ messages }}"));
    std::fs::write(root.join("chiral-model.json"), protocol::canonical_json(&config.model).unwrap()).unwrap();
    assert!(validate_inventory(&config).is_err());
    config.model.chat_template_sha256 = hex::encode(Sha256::digest(b""));
    std::fs::write(root.join("chiral-model.json"), protocol::canonical_json(&config.model).unwrap()).unwrap();
    assert!(validate_inventory(&config).unwrap().template.is_none());
}

#[test]
fn inventories_bind_every_selected_byte_and_reject_unlisted_libraries() {
    let (_directory, config) = fixture();
    let validated = validate_inventory(&config).unwrap();
    assert_eq!(validated.task, "generation");
    let runtime = config.runtime.as_ref().unwrap();
    std::fs::write(runtime.runtime_dir.join("unlisted.dll"), b"untrusted loader input").unwrap();
    assert!(validate_inventory(&config).err().unwrap().contains("unexpected file"));
    std::fs::remove_file(runtime.runtime_dir.join("unlisted.dll")).unwrap();
    std::fs::write(runtime.artifact_dir.join("model-Q4_K_M.gguf"), b"GGUF-test-inventory-byteX").unwrap();
    assert!(validate_inventory(&config).is_err());
}

#[test]
fn immutable_template_and_base_model_must_match_configuration() {
    let (_directory, mut config) = fixture();
    config.model.context_length += 1;
    assert!(validate_inventory(&config).err().unwrap().contains("ModelSpec"));
    config.model.context_length -= 1;
    config.model.chat_template_sha256 = "00".repeat(32);
    assert!(validate_inventory(&config).is_err());
}

#[test]
fn effective_template_preserves_whitespace_except_pinned_lexer_normalization() {
    for (source, expected) in [
        (" \r\n{{ messages }}\r\n\r\n", " \n{{ messages }}\n"),
        ("{{ messages }}\r", "{{ messages }}\n"),
        ("{{ messages }}\n ", "{{ messages }}\n "),
        ("", ""),
    ] {
        assert_eq!(template_digest(source.as_bytes()), hex::encode(Sha256::digest(expected.as_bytes())));
    }
}

#[test]
fn runtime_rejects_floating_identity_and_unsafe_inventory_paths() {
    let (_directory, config) = fixture();
    let root = &config.runtime.as_ref().unwrap().runtime_dir;
    let path = root.join("chiral-runtime.json");
    let mut manifest = read_json(&path).unwrap();
    manifest["source_revision"] = json!("master");
    std::fs::write(&path, protocol::canonical_json(&manifest).unwrap()).unwrap();
    assert!(validate_inventory(&config).err().unwrap().contains("pinned upstream"));
    for path in ["../engine", "/absolute", "C:\\engine.exe", "folder/../engine", "a\\b"] { assert!(relative_path(path).is_err()); }
}


#[test]
fn managed_endpoint_and_key_require_literal_loopback_and_single_protected_token() {
    let (_directory, mut config) = fixture();
    for endpoint in ["http://localhost:18080", "http://192.168.1.2:18080", "https://127.0.0.1:18080", "http://127.0.0.1"] {
        config.engine_url = endpoint.into();
        assert!(config.runtime.as_ref().unwrap().validate(&config).is_err());
    }
    config.engine_url = "http://[::1]:18080".into();
    assert!(config.runtime.as_ref().unwrap().validate(&config).is_ok());
    let key = config.engine_api_key_file.as_ref().unwrap();
    std::fs::write(key, b"#comment-is-not-an-engine-auth-key-123456789").unwrap();
    assert!(validate_inventory(&config).is_err());
    std::fs::write(key, b" test-private-engine-token-32-characters\n").unwrap();
    assert!(validate_inventory(&config).is_err());
}

#[cfg(unix)]
#[test]
fn symlink_inventory_entries_are_rejected_before_execution() {
    let (_directory, config) = fixture();
    let root = &config.runtime.as_ref().unwrap().artifact_dir;
    std::os::unix::fs::symlink("model-Q4_K_M.gguf", root.join("alias.gguf")).unwrap();
    assert!(validate_inventory(&config).err().unwrap().contains("symlink"));
}

#[test]
#[ignore = "native child helper, invoked only by lifecycle tests"]
fn managed_child_helper() { std::thread::sleep(Duration::from_secs(60)); }

fn child() -> ManagedEngine {
    ManagedEngine::helper_for_test("test/helper", "http://127.0.0.1:1")
}

#[tokio::test]
async fn shutdown_reaps_child_and_permanently_withdraws_readiness() {
    let engine = child();
    assert!(engine.is_alive());
    engine.stop().await;
    assert!(!engine.is_alive());
    assert!(engine.child.lock().unwrap().try_wait().unwrap().is_some());
    engine.stop().await;
    assert!(!engine.is_alive());
}

#[tokio::test]
async fn process_exit_is_observed_without_waiting_for_health_refresh() {
    let engine = child();
    engine.child.lock().unwrap().start_kill().unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while engine.is_alive() && tokio::time::Instant::now() < deadline { tokio::time::sleep(Duration::from_millis(20)).await; }
    assert!(!engine.is_alive());
}

#[tokio::test]
async fn startup_timeout_kills_child_instead_of_admitting_or_retrying() {
    let engine = child();
    engine.ready.store(false, Ordering::Release);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let result = engine.await_health(&url, "private-test-token", Duration::from_millis(50)).await;
    assert!(result.err().unwrap().contains("timed out"));
    assert!(!engine.is_alive());
    assert!(engine.child.lock().unwrap().try_wait().unwrap().is_some());
}

#[cfg(unix)]
#[tokio::test]
async fn dropping_runtime_does_not_strand_the_native_child() {
    let engine = child();
    let pid = engine.observation.pid as i32;
    drop(engine);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(pid, 0) } == 0 && tokio::time::Instant::now() < deadline { tokio::time::sleep(Duration::from_millis(20)).await; }
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

/// Errors `ManagedEngine::start` can return with the CPU fixture once the port check has
/// passed: its `llama-server` is only the first 4,096 bytes of this test binary.
fn failed_after_port_check(error: &str) -> bool {
    error != OCCUPIED_ENGINE_PORT
        && ["cannot start pinned native llama-server", "managed engine exited before readiness", "managed engine startup timed out"].contains(&error)
}

/// Another process's listener: an IPv6 one is `IPV6_V6ONLY`, so that it occupies only its family.
/// With `reuseaddr` it has `SO_REUSEADDR`, as `std::net::TcpListener` sets on Unix (task 2.2).
async fn foreign_listener(address: std::net::SocketAddr, reuseaddr: bool) -> tokio::net::TcpListener {
    let socket = if address.is_ipv4() { tokio::net::TcpSocket::new_v4() } else { tokio::net::TcpSocket::new_v6() }.unwrap();
    socket.set_reuseaddr(reuseaddr).unwrap();
    #[cfg(unix)]
    if address.is_ipv6() {
        use std::os::fd::AsRawFd;
        let on: libc::c_int = 1;
        assert_eq!(unsafe { libc::setsockopt(socket.as_raw_fd(), libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, (&on as *const libc::c_int).cast(), std::mem::size_of::<libc::c_int>() as libc::socklen_t) }, 0);
    }
    socket.bind(address).unwrap();
    socket.listen(8).unwrap()
}

async fn assert_still_accepts(listener: &tokio::net::TcpListener, endpoint: std::net::SocketAddr) {
    let client = tokio::net::TcpStream::connect(endpoint).await.unwrap();
    let (_accepted, peer) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
    assert_eq!(peer, client.local_addr().unwrap());
}

#[tokio::test]
async fn a_listener_on_the_engine_address_or_its_wildcard_is_refused_before_spawn() {
    let (_directory, mut config) = fixture();
    let mut cases = vec![("127.0.0.1:0", "127.0.0.1"), ("0.0.0.0:0", "127.0.0.1")];
    // Where IPv6 loopback exists, the same holds for `[::1]` beside `[::1]` and `[::]`.
    if std::net::TcpListener::bind("[::1]:0").is_ok() { cases.extend([("[::1]:0", "[::1]"), ("[::]:0", "[::1]")]); }
    // On Windows `SO_REUSEADDR` lets a socket share a port another one holds; not that case.
    let options: &[bool] = if cfg!(windows) { &[false] } else { &[false, true] };
    for (bound, host) in cases {
        for &reuseaddr in options {
            let listener = foreign_listener(bound.parse().unwrap(), reuseaddr).await;
            let port = listener.local_addr().unwrap().port();
            config.engine_url = format!("http://{host}:{port}");
            let error = ManagedEngine::start(&config).await.err().unwrap();
            assert_eq!(error, OCCUPIED_ENGINE_PORT, "{bound} (SO_REUSEADDR {reuseaddr}) occupies {}", config.engine_url);
            // Not adopted, signalled or disturbed: the other process still accepts connections.
            assert_still_accepts(&listener, format!("{host}:{port}").parse().unwrap()).await;
        }
    }
}

#[tokio::test]
async fn a_port_holding_only_time_wait_connections_passes_the_port_check() {
    use std::io::Read as _;
    let (_directory, mut config) = fixture();
    // `127.0.0.1`, and `[::1]` where IPv6 loopback exists (its probe is `IPV6_V6ONLY`).
    let mut hosts = vec!["127.0.0.1:0"];
    if std::net::TcpListener::bind("[::1]:0").is_ok() { hosts.push("[::1]:0"); }
    for host in hosts {
        let listener = std::net::TcpListener::bind(host).unwrap();
        let endpoint = listener.local_addr().unwrap();
        let mut client = std::net::TcpStream::connect(endpoint).unwrap();
        let (accepted, _) = listener.accept().unwrap();
        // The listening side closes first, so its end of the connection waits in TIME_WAIT on
        // the endpoint, as a stopped engine's keep-alive connections do.
        drop(accepted);
        assert_eq!(client.read(&mut [0u8; 1]).unwrap(), 0);
        drop(client);
        drop(listener);
        std::thread::sleep(Duration::from_millis(200));
        // The port is not free of sockets: a bind without `SO_REUSEADDR` is refused (measured
        // on macOS, design decision 5), so dropping the option would refuse a prompt restart.
        #[cfg(target_os = "macos")]
        assert!(if endpoint.is_ipv4() { tokio::net::TcpSocket::new_v4() } else { tokio::net::TcpSocket::new_v6() }.unwrap().bind(endpoint).is_err(), "no TIME_WAIT connection was left on {endpoint}");
        config.engine_url = format!("http://{endpoint}");
        let error = ManagedEngine::start(&config).await.err().unwrap();
        assert!(failed_after_port_check(&error), "{endpoint}: {error}");
    }
}

// managed-engine-runtime task 3.3: managed runtime settings are explicit, bounded and refused
// rather than adjusted (requirement 3).

const BOUNDS: &str = "managed engine startup, batch, or layer settings exceed bounds";
const CACHE_OR_SPLIT: &str = "unsupported managed engine cache or split mode";
const FLASH: &str = "quantized value cache requires flash attention";
const BACKEND: &str = "unsupported native engine backend on this platform";
const SELECTOR: &str = "invalid or duplicate explicit engine device selector";
const CPU: &str = "CPU runtime cannot select GPU execution";
const ACCELERATED: &str = "accelerated runtime requires explicit devices and offloaded layers";
const SPLIT: &str = "tensor split must describe exactly the selected devices";
const HEADROOM: &str = "managed runtime requires explicit platform memory headroom";
const CONTEXT: &str = "managed aggregate context exceeds engine bounds";

/// A development provider configuration whose only model is `model`, as an operator writes it.
fn provider_json(model: &ModelConfig) -> Value {
    json!({
        "network": {
            "identity": {"chain_id": crate::network::ESPACE_TESTNET_CHAIN_ID,
                "genesis_hash": crate::network::ESPACE_TESTNET_GENESIS, "deployment_id": "managed-engine-runtime-tests"},
            "rpc_url": "http://127.0.0.1:18545", "rpc_bearer_file": null, "faucet_url": null,
            "explorer_url": null, "policy_url": null
        },
        "data_dir": "data", "signing_key_file": "wallet.key", "admin_token_file": "admin.token",
        "public_endpoint": "http://127.0.0.1:18082", "public_bind": "127.0.0.1:18082", "admin_bind": "127.0.0.1:18083",
        "platform_wallet": "0x2222222222222222222222222222222222222222", "fee_bps": 50, "min_funding_wei": "1000",
        "models": [model],
        "dht": {"listen": "/ip4/127.0.0.1/tcp/0", "bootstrap": [], "identity_file": "dht.key"},
        "development": true
    })
}

/// Loads `value` as the daemon loads its configuration: from a file, with `ProviderConfig::load`.
fn load_provider(directory: &Path, value: &Value) -> Result<crate::provider_config::ProviderConfig, String> {
    let path = directory.join("provider.json");
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    crate::provider_config::ProviderConfig::load(&path)
}

fn runtime_of(model: &mut ModelConfig) -> &mut EngineRuntimeConfig { model.runtime.as_mut().unwrap() }

/// An accelerated `backend` runtime on `devices` with `reserve` MiB of headroom, every layer
/// offloaded and, for several devices, equal layer-split weights: valid unless `reserve` is low.
fn accelerated(model: &mut ModelConfig, backend: &str, devices: &[&str], reserve: u64) {
    let runtime = runtime_of(model);
    runtime.backend = backend.into();
    runtime.device_ids = devices.iter().map(|device| device.to_string()).collect();
    runtime.memory_reserve_mib = reserve;
    runtime.gpu_layers = 999;
    (runtime.split_mode, runtime.tensor_split) =
        if devices.len() > 1 { ("layer".into(), vec![1; devices.len()]) } else { ("none".into(), vec![]) };
}

fn cuda_selectors(count: usize) -> Vec<String> { (0..count).map(|n| format!("CUDA{n}")).collect() }

type SettingsCase = (String, Box<dyn Fn(&mut ModelConfig)>, Result<(), &'static str>);

fn case(name: &str, change: impl Fn(&mut ModelConfig) + 'static, expected: Result<(), &'static str>) -> SettingsCase {
    (name.into(), Box::new(change), expected)
}

/// Requirement 3 (task 3.3), table-driven: each runtime configuration is validated by
/// `EngineRuntimeConfig::validate` and loaded from a provider configuration file. A refused one
/// gets the same error from both; an accepted one loads with every setting exactly as written.
#[test]
fn runtime_settings_are_explicit_bounded_and_refused_rather_than_adjusted() {
    let (directory, base) = fixture();
    let mut cases = vec![
        case("the CPU fixture", |_| {}, Ok(())),
        // Dedicated devices: at least 256 MiB of headroom each.
        case("CUDA with a 256 MiB reserve", |m| accelerated(m, "cuda", &["CUDA0"], 256), Ok(())),
        case("CUDA with a 255 MiB reserve", |m| accelerated(m, "cuda", &["CUDA0"], 255), Err(HEADROOM)),
        case("two CUDA devices with a 255 MiB reserve", |m| accelerated(m, "cuda", &["CUDA0", "CUDA1"], 255), Err(HEADROOM)),
        case("CUDA without a reserve", |m| accelerated(m, "cuda", &["CUDA0"], 0), Err(HEADROOM)),
        case("Vulkan with a 256 MiB reserve", |m| accelerated(m, "vulkan", &["Vulkan0"], 256), Ok(())),
        case("Vulkan with a 255 MiB reserve", |m| accelerated(m, "vulkan", &["Vulkan0"], 255), Err(HEADROOM)),
        // A CPU runtime cannot select GPU execution.
        case("cpu with CUDA0", |m| runtime_of(m).device_ids = vec!["CUDA0".into()], Err(SELECTOR)),
        case("cpu with a bare device index", |m| runtime_of(m).device_ids = vec!["0".into()], Err(CPU)),
        case("cpu offloading one layer", |m| runtime_of(m).gpu_layers = 1, Err(CPU)),
        case("cpu with split mode layer", |m| runtime_of(m).split_mode = "layer".into(), Err(CPU)),
        case("cpu with a split weight", |m| runtime_of(m).tensor_split = vec![1], Err(CPU)),
        // An accelerated backend selects devices and offloads 1 to 999 layers.
        case("CUDA without a device", |m| { accelerated(m, "cuda", &[], 256); }, Err(ACCELERATED)),
        case("Vulkan without a device", |m| { accelerated(m, "vulkan", &[], 256); }, Err(ACCELERATED)),
        case("CUDA offloading no layer", |m| { accelerated(m, "cuda", &["CUDA0"], 256); runtime_of(m).gpu_layers = 0; }, Err(ACCELERATED)),
        case("CUDA offloading one layer", |m| { accelerated(m, "cuda", &["CUDA0"], 256); runtime_of(m).gpu_layers = 1; }, Ok(())),
        case("CUDA offloading 1,000 layers", |m| { accelerated(m, "cuda", &["CUDA0"], 256); runtime_of(m).gpu_layers = 1000; }, Err(BOUNDS)),
        // Explicit, unique selectors of the backend's form; at most 16 (design decision 9).
        case("a duplicate selector", |m| accelerated(m, "cuda", &["CUDA0", "CUDA0"], 256), Err(SELECTOR)),
        case("selector CUDA", |m| accelerated(m, "cuda", &["CUDA"], 256), Err(SELECTOR)),
        case("selector CUDAx", |m| accelerated(m, "cuda", &["CUDAx"], 256), Err(SELECTOR)),
        case("selector cuda0", |m| accelerated(m, "cuda", &["cuda0"], 256), Err(SELECTOR)),
        case("Vulkan0 under cuda", |m| accelerated(m, "cuda", &["Vulkan0"], 256), Err(SELECTOR)),
        case("MTL0 under cuda", |m| accelerated(m, "cuda", &["MTL0"], 256), Err(SELECTOR)),
        case("CUDA0 under vulkan", |m| accelerated(m, "vulkan", &["CUDA0"], 256), Err(SELECTOR)),
        // Only ASCII digits follow the prefix: no sign, space or second selector in one entry.
        case("selector CUDA+1", |m| accelerated(m, "cuda", &["CUDA+1"], 256), Err(SELECTOR)),
        case("selector CUDA-1", |m| accelerated(m, "cuda", &["CUDA-1"], 256), Err(SELECTOR)),
        case("selector CUDA 0", |m| accelerated(m, "cuda", &["CUDA 0"], 256), Err(SELECTOR)),
        case("one selector CUDA0,CUDA1", |m| accelerated(m, "cuda", &["CUDA0,CUDA1"], 256), Err(SELECTOR)),
        case("16 selectors", |m| { let ids = cuda_selectors(16); accelerated(m, "cuda", &ids.iter().map(String::as_str).collect::<Vec<_>>(), 256); }, Ok(())),
        case("17 selectors", |m| { let ids = cuda_selectors(17); accelerated(m, "cuda", &ids.iter().map(String::as_str).collect::<Vec<_>>(), 256); }, Err(SELECTOR)),
        // Split weights describe exactly the selected devices.
        case("one device with split mode layer", |m| { accelerated(m, "cuda", &["CUDA0"], 256); runtime_of(m).split_mode = "layer".into(); }, Err(SPLIT)),
        case("one device with a weight", |m| { accelerated(m, "cuda", &["CUDA0"], 256); runtime_of(m).tensor_split = vec![1]; }, Err(SPLIT)),
        case("two devices, layer, weights [3, 1]", |m| { accelerated(m, "cuda", &["CUDA0", "CUDA1"], 256); runtime_of(m).tensor_split = vec![3, 1]; }, Ok(())),
        case("two devices, row, weights [1, 1]", |m| { accelerated(m, "cuda", &["CUDA0", "CUDA1"], 256); runtime_of(m).split_mode = "row".into(); }, Ok(())),
        case("two devices without weights", |m| { accelerated(m, "cuda", &["CUDA0", "CUDA1"], 256); runtime_of(m).tensor_split = vec![]; }, Err(SPLIT)),
        case("two devices with three weights", |m| { accelerated(m, "cuda", &["CUDA0", "CUDA1"], 256); runtime_of(m).tensor_split = vec![1, 1, 1]; }, Err(SPLIT)),
        case("two devices with a zero weight", |m| { accelerated(m, "cuda", &["CUDA0", "CUDA1"], 256); runtime_of(m).tensor_split = vec![1, 0]; }, Err(SPLIT)),
        case("two devices with split mode none", |m| { accelerated(m, "cuda", &["CUDA0", "CUDA1"], 256); runtime_of(m).split_mode = "none".into(); }, Err(SPLIT)),
        case("split mode tensor", |m| { accelerated(m, "cuda", &["CUDA0", "CUDA1"], 256); runtime_of(m).split_mode = "tensor".into(); }, Err(CACHE_OR_SPLIT)),
        // A quantized value cache needs flash attention; it is refused, not replaced.
        case("value cache q4_0 without flash attention", |m| runtime_of(m).cache_type_v = "q4_0".into(), Err(FLASH)),
        case("value cache q8_0 without flash attention", |m| runtime_of(m).cache_type_v = "q8_0".into(), Err(FLASH)),
        case("value cache q4_0 with flash attention", |m| { runtime_of(m).cache_type_v = "q4_0".into(); runtime_of(m).flash_attention = true; }, Ok(())),
        case("key cache q8_0 without flash attention", |m| runtime_of(m).cache_type_k = "q8_0".into(), Ok(())),
        case("key cache q6_K", |m| runtime_of(m).cache_type_k = "q6_K".into(), Err(CACHE_OR_SPLIT)),
        case("value cache F16", |m| { runtime_of(m).cache_type_v = "F16".into(); runtime_of(m).flash_attention = true; }, Err(CACHE_OR_SPLIT)),
        // Batch 1 to 8,192, microbatch 1 to the batch.
        case("batch and microbatch 1", |m| { runtime_of(m).batch_size = 1; runtime_of(m).ubatch_size = 1; }, Ok(())),
        case("batch and microbatch 8,192", |m| { runtime_of(m).batch_size = 8192; runtime_of(m).ubatch_size = 8192; }, Ok(())),
        case("batch 0", |m| runtime_of(m).batch_size = 0, Err(BOUNDS)),
        case("batch 8,193", |m| runtime_of(m).batch_size = 8193, Err(BOUNDS)),
        case("microbatch 0", |m| runtime_of(m).ubatch_size = 0, Err(BOUNDS)),
        case("microbatch above the batch", |m| runtime_of(m).ubatch_size = 129, Err(BOUNDS)),
        // Startup timeout 1 to 1,800 seconds (design decision 9).
        case("startup timeout 1,800", |m| runtime_of(m).startup_timeout_secs = 1800, Ok(())),
        case("startup timeout 0", |m| runtime_of(m).startup_timeout_secs = 0, Err(BOUNDS)),
        case("startup timeout 1,801", |m| runtime_of(m).startup_timeout_secs = 1801, Err(BOUNDS)),
        // Only the listed backends.
        case("backend rocm", |m| runtime_of(m).backend = "rocm".into(), Err(BACKEND)),
        case("backend CUDA", |m| runtime_of(m).backend = "CUDA".into(), Err(BACKEND)),
        case("an empty backend", |m| runtime_of(m).backend = String::new(), Err(BACKEND)),
    ];
    for cache in ["f32", "f16", "bf16", "q8_0", "q4_0", "q4_1", "iq4_nl", "q5_0", "q5_1"] {
        cases.push(case(&format!("key and value cache {cache} with flash attention"), move |m| {
            let runtime = runtime_of(m);
            (runtime.cache_type_k, runtime.cache_type_v, runtime.flash_attention) = (cache.into(), cache.into(), true);
        }, Ok(())));
    }
    // Unified memory: Metal on macOS only, exactly one device and at least 4,096 MiB of headroom.
    #[cfg(target_os = "macos")]
    cases.extend([
        case("Metal with a 4,096 MiB reserve", |m| accelerated(m, "metal", &["MTL0"], 4096), Ok(())),
        case("Metal with a 4,095 MiB reserve", |m| accelerated(m, "metal", &["MTL0"], 4095), Err(HEADROOM)),
        case("Metal on MTL0 and MTL1", |m| accelerated(m, "metal", &["MTL0", "MTL1"], 4096), Err(HEADROOM)),
        case("Metal without a device", |m| accelerated(m, "metal", &[], 4096), Err(ACCELERATED)),
        case("CUDA0 under metal", |m| accelerated(m, "metal", &["CUDA0"], 4096), Err(SELECTOR)),
    ]);
    #[cfg(not(target_os = "macos"))]
    cases.push(case("Metal off macOS", |m| accelerated(m, "metal", &["MTL0"], 4096), Err(BACKEND)));

    for (name, change, expected) in &cases {
        let mut model = base.clone();
        change(&mut model);
        assert_eq!(model.runtime.as_ref().unwrap().validate(&model), expected.map_err(String::from), "{name}");
        let loaded = load_provider(directory.path(), &provider_json(&model));
        match expected {
            Ok(()) => {
                let loaded = loaded.unwrap_or_else(|error| panic!("{name}: {error}"));
                // Accepted as written: no setting was replaced by another value.
                assert_eq!(serde_json::to_value(&loaded.models[0].runtime).unwrap(), serde_json::to_value(&model.runtime).unwrap(), "{name}");
            }
            Err(error) => assert_eq!(loaded.err().as_deref(), Some(*error), "{name}: configuration loading"),
        }
    }

    // The total context, context length times concurrency, fits the engine's bound of
    // 2,147,483,647 tokens (design decision 9), without overflow.
    let mut model = base.clone();
    model.max_concurrency = 2;
    for (context, expected) in [(1_073_741_823, Ok(())), (1_073_741_824, Err(CONTEXT.to_owned())), (u64::MAX, Err(CONTEXT.to_owned()))] {
        model.model.context_length = context;
        assert_eq!(model.runtime.as_ref().unwrap().validate(&model), expected, "context {context} at concurrency 2");
    }
    // The bound itself is inclusive: 2,147,483,647 is prime, so only concurrency 1 reaches it.
    model.max_concurrency = 1;
    for (context, expected) in [(2_147_483_647, Ok(())), (2_147_483_648, Err(CONTEXT.to_owned()))] {
        model.model.context_length = context;
        assert_eq!(model.runtime.as_ref().unwrap().validate(&model), expected, "context {context} at concurrency 1");
    }
    model.max_concurrency = 2;
    // Loading refuses such a model earlier: a model context is at most 1,048,576 tokens.
    model.model.context_length = 1_073_741_824;
    assert!(load_provider(directory.path(), &provider_json(&model)).is_err());

    // Exactly the fourteen fields: no unknown field, such as an engine option, and none defaulted.
    let fields = ["runtime_dir", "artifact_dir", "backend", "device_ids", "memory_reserve_mib", "gpu_layers", "split_mode",
        "tensor_split", "cache_type_k", "cache_type_v", "flash_attention", "batch_size", "ubatch_size", "startup_timeout_secs"];
    let written = serde_json::to_value(base.runtime.as_ref().unwrap()).unwrap();
    assert_eq!(written.as_object().unwrap().keys().map(String::as_str).collect::<HashSet<_>>(), fields.into_iter().collect());
    let mut value = provider_json(&base);
    value["models"][0]["runtime"]["extra_args"] = json!(["--host", "0.0.0.0"]);
    let error = load_provider(directory.path(), &value).err().unwrap();
    assert!(error.contains("unknown field `extra_args`"), "{error}");
    for field in fields {
        let mut value = provider_json(&base);
        value["models"][0]["runtime"].as_object_mut().unwrap().remove(field);
        let error = load_provider(directory.path(), &value).err().unwrap();
        assert!(error.contains(&format!("missing field `{field}`")), "{field}: {error}");
    }
}

// managed-engine-runtime task 3.5: the launch passes the serving contract to the engine as
// explicit arguments (requirement 5).

/// The engine key of `fixture`.
const FIXTURE_KEY: &str = "test-private-engine-token-32-characters";

/// The launch command built from `config`'s verified inventory: its program, its arguments and
/// its working directory.
fn launch_of(config: &ModelConfig) -> (PathBuf, Vec<String>, Option<PathBuf>) {
    let validated = validate_inventory(config).unwrap();
    let launch = launch_command(config, &validated).unwrap();
    let launch = launch.as_std();
    let args = launch.get_args().map(|arg| arg.to_str().unwrap().to_owned()).collect();
    (PathBuf::from(launch.get_program()), args, launch.get_current_dir().map(Path::to_path_buf))
}

fn value_of<'a>(args: &'a [String], option: &str) -> Option<&'a str> {
    args.iter().position(|arg| arg == option).and_then(|index| args.get(index + 1)).map(String::as_str)
}

fn canonical(path: &Path) -> String { path.canonicalize().unwrap().to_str().unwrap().to_owned() }

/// What requirement 5 forbids in any launch: an option naming a remote model source, enabling
/// automatic fitting or context shifting, opening another listener or carrying a secret value,
/// and an option given twice.
fn assert_only_the_contract(args: &[String]) {
    let options: Vec<&str> = args.iter().map(String::as_str).filter(|arg| arg.starts_with('-')).collect();
    assert_eq!(options.iter().collect::<HashSet<_>>().len(), options.len(), "an option is repeated: {args:?}");
    for forbidden in ["-hf", "-hfr", "--hf-repo", "-hff", "--hf-file", "-hft", "--hf-token", "-mu", "--model-url",
        "-dr", "--docker-repo", "--api-key", "--context-shift", "--webui", "--mmproj", "--mmproj-url", "--rpc"] {
        assert!(!options.contains(&forbidden), "{forbidden} in {args:?}");
    }
    assert!(args.iter().all(|arg| !arg.contains("://")), "an argument names a remote source: {args:?}");
    assert!(args.iter().all(|arg| !arg.contains(FIXTURE_KEY)), "an argument carries the engine key");
    assert_eq!((value_of(args, "--fit"), options.contains(&"--no-context-shift")), (Some("off"), true));
}

#[test]
fn launch_command_passes_the_serving_contract_as_explicit_arguments() {
    let (_directory, mut config) = fixture();
    let runtime = config.runtime.clone().unwrap();
    let executable = runtime.runtime_dir.join(if cfg!(windows) { "llama-server.exe" } else { "llama-server" }).canonicalize().unwrap();
    let model = canonical(&runtime.artifact_dir.join("model-Q4_K_M.gguf"));
    let template = canonical(&runtime.artifact_dir.join("chiral-chat-template.jinja"));
    let key = canonical(config.engine_api_key_file.as_ref().unwrap());

    // The CPU chat fixture: context 512, concurrency 2, output 64. The complete current vector,
    // with the disabling options of design decision 9.
    let (program, args, directory) = launch_of(&config);
    assert_eq!(program, executable);
    assert_eq!(directory, Some(runtime.runtime_dir.canonicalize().unwrap()));
    assert_eq!(args, [
        "--model", model.as_str(), "--alias", "test/repo/model-Q4_K_M.gguf", "--host", "127.0.0.1", "--port", "18080",
        "--api-key-file", key.as_str(), "--ctx-size", "1024", "--kv-unified-per-slot", "512", "--parallel", "2", "--n-predict", "64",
        "--no-context-shift", "--no-cache-prompt", "--cache-reuse", "0", "--cache-ram", "0", "--fit", "off",
        "--no-webui", "--no-mmproj", "--no-prefill-assistant", "--log-disable",
        "--device", "none", "--gpu-layers", "0", "--split-mode", "none", "--cache-type-k", "f16", "--cache-type-v", "f16",
        "--flash-attn", "off", "--batch-size", "128", "--ubatch-size", "64", "--jinja", "--chat-template-file", template.as_str(),
    ]);
    assert_only_the_contract(&args);
    assert!(!args.iter().any(|arg| arg == "--tensor-split" || arg == "--embedding"));

    // A bracketed IPv6 loopback endpoint: the host without brackets.
    config.engine_url = "http://[::1]:18080".into();
    let (_, args, _) = launch_of(&config);
    assert_eq!((value_of(&args, "--host"), value_of(&args, "--port")), (Some("::1"), Some("18080")));
    assert_only_the_contract(&args);

    // Two CUDA devices (manifest backend `cuda`), split `layer` with weights [3, 1], 999 layers,
    // flash attention on; quantized caches, other batch sizes and concurrency 3.
    let manifest_path = runtime.runtime_dir.join("chiral-runtime.json");
    let mut manifest = read_json(&manifest_path).unwrap();
    manifest["backend"] = json!("cuda");
    std::fs::write(&manifest_path, protocol::canonical_json(&manifest).unwrap()).unwrap();
    config.engine_url = "http://127.0.0.1:18080".into();
    config.max_concurrency = 3;
    accelerated(&mut config, "cuda", &["CUDA0", "CUDA1"], 256);
    let cuda = runtime_of(&mut config);
    (cuda.tensor_split, cuda.flash_attention, cuda.cache_type_k, cuda.cache_type_v) = (vec![3, 1], true, "q8_0".into(), "q4_0".into());
    (cuda.batch_size, cuda.ubatch_size) = (512, 256);
    let (program, args, directory) = launch_of(&config);
    assert_eq!((program.as_path(), directory.as_deref()), (executable.as_path(), executable.parent()));
    assert_eq!(args, [
        "--model", model.as_str(), "--alias", "test/repo/model-Q4_K_M.gguf", "--host", "127.0.0.1", "--port", "18080",
        "--api-key-file", key.as_str(), "--ctx-size", "1536", "--kv-unified-per-slot", "512", "--parallel", "3", "--n-predict", "64",
        "--no-context-shift", "--no-cache-prompt", "--cache-reuse", "0", "--cache-ram", "0", "--fit", "off",
        "--no-webui", "--no-mmproj", "--no-prefill-assistant", "--log-disable",
        "--device", "CUDA0,CUDA1", "--gpu-layers", "999", "--split-mode", "layer", "--cache-type-k", "q8_0", "--cache-type-v", "q4_0",
        "--flash-attn", "on", "--batch-size", "512", "--ubatch-size", "256", "--tensor-split", "3,1",
        "--jinja", "--chat-template-file", template.as_str(),
    ]);
    assert_only_the_contract(&args);

    // One CUDA device: split mode `none`, and no `--tensor-split`, which is passed only when
    // several devices are selected.
    accelerated(&mut config, "cuda", &["CUDA0"], 256);
    let (_, args, _) = launch_of(&config);
    assert_eq!(args, [
        "--model", model.as_str(), "--alias", "test/repo/model-Q4_K_M.gguf", "--host", "127.0.0.1", "--port", "18080",
        "--api-key-file", key.as_str(), "--ctx-size", "1536", "--kv-unified-per-slot", "512", "--parallel", "3", "--n-predict", "64",
        "--no-context-shift", "--no-cache-prompt", "--cache-reuse", "0", "--cache-ram", "0", "--fit", "off",
        "--no-webui", "--no-mmproj", "--no-prefill-assistant", "--log-disable",
        "--device", "CUDA0", "--gpu-layers", "999", "--split-mode", "none", "--cache-type-k", "q8_0", "--cache-type-v", "q4_0",
        "--flash-attn", "on", "--batch-size", "512", "--ubatch-size", "256",
        "--jinja", "--chat-template-file", template.as_str(),
    ]);
    assert_only_the_contract(&args);

    // An embeddings artifact: `--embedding`, and neither `--jinja` nor `--chat-template-file`.
    let (_directory, embedding) = embedding_fixture();
    let artifact = &embedding.runtime.as_ref().unwrap().artifact_dir;
    let (_, args, _) = launch_of(&embedding);
    assert_eq!(args, [
        "--model", canonical(&artifact.join("model-Q4_K_M.gguf")).as_str(), "--alias", "test/repo/model-Q4_K_M.gguf",
        "--host", "127.0.0.1", "--port", "18080", "--api-key-file", canonical(embedding.engine_api_key_file.as_ref().unwrap()).as_str(),
        "--ctx-size", "1024", "--kv-unified-per-slot", "512", "--parallel", "2", "--n-predict", "0",
        "--no-context-shift", "--no-cache-prompt", "--cache-reuse", "0", "--cache-ram", "0", "--fit", "off",
        "--no-webui", "--no-mmproj", "--no-prefill-assistant", "--log-disable",
        "--device", "none", "--gpu-layers", "0", "--split-mode", "none", "--cache-type-k", "f16", "--cache-type-v", "f16",
        "--flash-attn", "off", "--batch-size", "128", "--ubatch-size", "64", "--embedding",
    ]);
    assert_only_the_contract(&args);
    assert!(!args.iter().any(|arg| arg == "--jinja" || arg == "--chat-template-file"));
}

// managed-engine-runtime task 3.6: the child receives a minimal environment and its API key only
// as a protected file path (requirement 6).

/// The extra test-name filter that marks a run of a launch-environment helper. It names no
/// test, so with `--exact` only the named helper runs; a helper run without it (as by
/// `cargo test -- --ignored`) does nothing.
const LAUNCH_PROBE: &str = "chiral-launch-environment-probe";

fn helper_args(helper: &'static str) -> [&'static str; 5] { ["--exact", helper, "--ignored", "--nocapture", LAUNCH_PROBE] }

fn probing() -> bool { std::env::args().any(|arg| arg == LAUNCH_PROBE) }

#[test]
#[ignore = "launch-environment child helper, run only by the_engine_child_receives_only_the_allowlisted_environment"]
fn launch_environment_child_helper() {
    if !probing() { return; }
    for (name, value) in std::env::vars_os() { println!("child-env\t{}={}", name.to_string_lossy(), value.to_string_lossy()); }
    println!("child-cwd\t{}", std::env::current_dir().unwrap().display());
}

/// The exit status of `launch_stdio_child_helper` when all three streams are `/dev/null`. A
/// distinct status, not success, so that a run in which the helper did not run (a filter that
/// matches no test exits with success) is not taken for a pass.
#[cfg(unix)]
const STDIO_NULL_EXIT: i32 = 73;

/// Exits with `STDIO_NULL_EXIT` only when standard input, output and error are all `/dev/null`.
#[cfg(unix)]
#[test]
#[ignore = "launch-stdio child helper, run only by the_engine_child_receives_only_the_allowlisted_environment"]
fn launch_stdio_child_helper() {
    use std::os::unix::fs::MetadataExt;
    if !probing() { return; }
    let null = std::fs::metadata("/dev/null").unwrap();
    for descriptor in 0..3 {
        // SAFETY: an all-zero `stat` is valid, and `fstat` writes one for the descriptor.
        let mut status: libc::stat = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::fstat(descriptor, &mut status) }, 0, "descriptor {descriptor} is not open");
        assert_eq!((status.st_dev as u64, status.st_ino as u64), (null.dev(), null.ino()), "descriptor {descriptor} is not /dev/null");
    }
    std::process::exit(STDIO_NULL_EXIT);
}

/// The daemon stand-in: runs with the polluted environment and the piped standard streams the
/// outer test gives it, starts the child helpers with the launcher's own process settings
/// (`command`, and `capture` as device observation does) and reports what they saw.
#[tokio::test]
#[ignore = "launch-environment parent helper, run only by the_engine_child_receives_only_the_allowlisted_environment"]
async fn launch_environment_parent_helper() {
    if !probing() { return; }
    for (name, value) in std::env::vars_os() { println!("parent-env\t{}={}", name.to_string_lossy(), value.to_string_lossy()); }
    let executable = std::env::current_exe().unwrap();
    let mut launch = command(&executable);
    // Standard output is piped only to read the report; the stdio helper below checks the default.
    launch.args(helper_args("engine_runtime::tests::launch_environment_child_helper")).stdout(Stdio::piped());
    let output = launch.spawn().unwrap().wait_with_output().await.unwrap();
    println!("launch-status\t{}", output.status.success());
    for line in String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.starts_with("child-")) { println!("launch:{line}"); }
    let observed = capture(&executable, &helper_args("engine_runtime::tests::launch_environment_child_helper")).await;
    println!("observer-status\t{}", observed.is_ok());
    for line in observed.unwrap_or_default().lines().filter(|line| line.starts_with("child-")) { println!("observer:{line}"); }
    #[cfg(unix)]
    {
        let status = command(&executable).args(helper_args("engine_runtime::tests::launch_stdio_child_helper")).status().await.unwrap();
        println!("stdio-exit\t{:?}", status.code());
    }
}

/// Requirement 6's environment scenario, without changing this test process's environment: a
/// parent helper runs with a polluted environment, and the children it starts through the
/// launcher, for the engine and for device observation, see exactly the allowlist.
#[test]
fn the_engine_child_receives_only_the_allowlisted_environment() {
    let directory = tempfile::tempdir().unwrap();
    let temporary = directory.path().join("daemon-tmp");
    std::fs::create_dir(&temporary).unwrap();
    let temporary = temporary.to_str().unwrap().to_owned();
    let home = directory.path().to_str().unwrap().to_owned();
    let mut polluted: Vec<(&str, &str)> = vec![
        ("LLAMA_ARG_CTX_SIZE", "1"), ("LLAMA_ARG_N_GPU_LAYERS", "0"), ("GGML_VK_VISIBLE_DEVICES", "1"), ("CUDA_VISIBLE_DEVICES", "1"),
        ("HTTP_PROXY", "http://127.0.0.1:9"), ("HTTPS_PROXY", "http://127.0.0.1:9"), ("ALL_PROXY", "socks5://127.0.0.1:9"), ("NO_PROXY", "*"),
        ("LD_PRELOAD", "/nonexistent/chiral-preload.so"), ("LD_LIBRARY_PATH", "/nonexistent/chiral-lib"), ("DYLD_LIBRARY_PATH", "/nonexistent/chiral-lib"),
        // dyld ends a process whose inserted library cannot load, so name one that exists there.
        ("DYLD_INSERT_LIBRARIES", if cfg!(target_os = "macos") { "/usr/lib/libSystem.B.dylib" } else { "/nonexistent/chiral-insert.dylib" }),
        ("HF_TOKEN", "hf_private_test_token"), ("CHIRAL_PROVIDER_CONFIG", "/nonexistent/provider.json"),
        ("PATH", "/usr/bin:/bin"), ("HOME", home.as_str()), ("TMPDIR", temporary.as_str()),
        // A value the launcher sets itself: the inherited one must not override it, because
        // CUDA selectors are matched to the signed physical devices in PCI bus order.
        ("CUDA_DEVICE_ORDER", "FASTEST_FIRST"),
    ];
    // Windows names are case-insensitive: there the lower-case proxy names would replace the
    // upper-case ones.
    if !cfg!(windows) {
        polluted.extend([("http_proxy", "http://127.0.0.1:9"), ("https_proxy", "http://127.0.0.1:9"),
            ("all_proxy", "socks5://127.0.0.1:9"), ("no_proxy", "*")]);
    }
    let mut parent = std::process::Command::new(std::env::current_exe().unwrap());
    parent.args(helper_args("engine_runtime::tests::launch_environment_parent_helper")).env_clear().envs(polluted.iter().copied())
        // Pipes, not `/dev/null`, so that a child inheriting a stream is told apart.
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // On Unix exactly these two; on Windows also the loader and temporary-directory variables.
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut expected = BTreeMap::from([("CUDA_DEVICE_ORDER".to_owned(), "PCI_BUS_ID".to_owned()), ("TMPDIR".to_owned(), temporary.clone())]);
    #[cfg(windows)]
    for name in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            parent.env(name, &value);
            expected.insert(name.to_uppercase(), value.into_string().unwrap());
        }
    }
    let output = parent.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "parent helper failed:\n{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
    let section = |prefix: &str| -> BTreeMap<String, String> {
        stdout.lines().filter_map(|line| line.strip_prefix(prefix)).filter_map(|entry| entry.split_once('='))
            .map(|(name, value)| (if cfg!(windows) { name.to_uppercase() } else { name.to_owned() }, value.to_owned())).collect()
    };
    // The pollution reached the daemon stand-in.
    let parent_environment = section("parent-env\t");
    for (name, value) in &polluted {
        let name = if cfg!(windows) { name.to_uppercase() } else { name.to_string() };
        assert_eq!(parent_environment.get(&name).map(String::as_str), Some(*value), "{name} did not reach the parent helper");
    }
    for status in ["launch-status\ttrue", "observer-status\ttrue"] { assert!(stdout.lines().any(|line| line == status), "{status}:\n{stdout}"); }
    for (process, prefix) in [("the engine launch", "launch:child-env\t"), ("a device observer", "observer:child-env\t")] {
        let mut environment = section(prefix);
        // CoreFoundation, which this test binary links, sets this variable in its own process
        // when it is absent; the launcher does not pass it. Allowed by name only.
        if cfg!(target_os = "macos") { environment.remove("__CF_USER_TEXT_ENCODING"); }
        assert_eq!(environment, expected, "the environment of {process}");
    }
    // The working directory is the executable's directory.
    let working = stdout.lines().find_map(|line| line.strip_prefix("launch:child-cwd\t")).unwrap();
    assert_eq!(Path::new(working).canonicalize().unwrap(), std::env::current_exe().unwrap().parent().unwrap().canonicalize().unwrap());
    // Standard input, output and error are `/dev/null`, although the daemon's are pipes.
    #[cfg(unix)]
    assert!(stdout.lines().any(|line| line == format!("stdio-exit\tSome({STDIO_NULL_EXIT})")), "an engine stream is not /dev/null:\n{stdout}");
}

#[test]
fn the_launch_command_sets_only_the_allowlisted_environment() {
    let (_directory, config) = fixture();
    let launch = launch_command(&config, &validate_inventory(&config).unwrap()).unwrap();
    let environment: BTreeMap<String, Option<String>> = launch.as_std().get_envs()
        .map(|(name, value)| (name.to_str().unwrap().to_owned(), value.map(|value| value.to_str().unwrap().to_owned()))).collect();
    let mut expected = BTreeMap::from([("CUDA_DEVICE_ORDER".to_owned(), Some("PCI_BUS_ID".to_owned()))]);
    for name in ["SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(name) { expected.insert(name.to_owned(), Some(value.into_string().unwrap())); }
    }
    assert_eq!(environment, expected);
    assert!(environment.values().flatten().all(|value| !value.contains(FIXTURE_KEY)));
}

/// Requirement 6's key file: a protected regular file of 1 to 4,096 bytes, opened without
/// following a link, holding one literal token of at least 32 characters, not beginning with
/// `#`, optionally followed by one newline. Each refusal comes from inventory verification, the
/// first step of `ManagedEngine::start`, so no engine child starts, and names no key byte.
#[tokio::test]
async fn engine_key_files_must_hold_one_protected_literal_token() {
    let (directory, mut config) = fixture();
    let key = config.engine_api_key_file.clone().unwrap();
    let token = "0123456789abcdefghijklmnopqrstuv";
    assert_eq!(token.len(), 32);
    let short = &token[..31];
    let long = "k".repeat(4096);
    let cases: Vec<(String, Vec<u8>, bool)> = vec![
        ("a 32-character token and one newline".into(), format!("{token}\n").into_bytes(), true),
        ("a 32-character token".into(), token.into(), true),
        ("a 4,096-byte token".into(), long.clone().into_bytes(), true),
        ("a 4,095-character token and one newline".into(), format!("{}\n", &long[..4095]).into_bytes(), true),
        ("a 31-character token".into(), format!("{short}\n").into_bytes(), false),
        ("a 31-character token without newline".into(), short.into(), false),
        ("two lines".into(), format!("{token}\n{token}x\n").into_bytes(), false),
        ("a token, CR LF and another line".into(), format!("{token}\r\nanother-line\n").into_bytes(), false),
        ("a token and CR LF".into(), format!("{token}\r\n").into_bytes(), false),
        ("a token and two newlines".into(), format!("{token}\n\n").into_bytes(), false),
        ("a token and a space".into(), format!("{token} ").into_bytes(), false),
        ("a token after a space".into(), format!(" {token}\n").into_bytes(), false),
        ("a token beginning with #".into(), format!("#{token}\n").into_bytes(), false),
        ("an empty file".into(), Vec::new(), false),
        ("4,097 bytes".into(), format!("{long}\n").into_bytes(), false),
    ];
    let refused = |error: &str| -> String {
        assert!(!error.contains(token) && !error.contains(short), "the error carries key bytes: {error}");
        error.to_owned()
    };
    for (name, bytes, accepted) in cases {
        std::fs::write(&key, &bytes).unwrap();
        let verified = validate_inventory(&config);
        assert_eq!(verified.is_ok(), accepted, "{name}");
        if let Err(error) = verified {
            assert_eq!(ManagedEngine::start(&config).await.err().unwrap(), refused(&error), "{name}: not refused before the launch");
        }
    }
    std::fs::write(&key, format!("{token}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // No group or other bit at all: read, write (another user could replace the key) or execute.
        for (mode, accepted) in [(0o644, false), (0o640, false), (0o604, false), (0o620, false), (0o602, false),
            (0o610, false), (0o601, false), (0o400, true), (0o600, true)] {
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(mode)).unwrap();
            let verified = validate_inventory(&config);
            assert_eq!(verified.is_ok(), accepted, "mode {mode:o}");
            if let Err(error) = verified { assert_eq!(ManagedEngine::start(&config).await.err().unwrap(), refused(&error), "mode {mode:o}"); }
        }
        // A symbolic link to that valid, protected key file.
        let link = directory.path().join("engine-link.key");
        std::os::unix::fs::symlink(&key, &link).unwrap();
        config.engine_api_key_file = Some(link);
        let error = refused(&validate_inventory(&config).err().unwrap());
        assert_eq!(ManagedEngine::start(&config).await.err().unwrap(), error, "symbolic link");
    }
    config.engine_api_key_file = Some(directory.path().join("missing.key"));
    let error = refused(&validate_inventory(&config).err().unwrap());
    assert_eq!(ManagedEngine::start(&config).await.err().unwrap(), error, "missing file");
    config.engine_api_key_file = None;
    assert_eq!(validate_inventory(&config).err().as_deref(), Some("managed engine requires a protected API key file"));
    config.engine_api_key_file = Some(key);
    assert!(validate_inventory(&config).is_ok());
}
