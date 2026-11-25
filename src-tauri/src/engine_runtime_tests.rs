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
        runtime:Some(EngineRuntimeConfig { strata_catalog: None, runtime_dir:runtime, artifact_dir:artifact, backend:"cpu".into(), device_ids:vec![], memory_reserve_mib:0, gpu_layers:0, split_mode:"none".into(), tensor_split:vec![], cache_type_k:"f16".into(), cache_type_v:"f16".into(), flash_attention:false, batch_size:128, ubatch_size:64, startup_timeout_secs:1 }) };
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

pub(super) fn helper_args(helper: &'static str) -> [&'static str; 5] { ["--exact", helper, "--ignored", "--nocapture", LAUNCH_PROBE] }

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

// Shared by the tests of tasks 3.1, 3.2, 3.4 and 3.7.

/// Rewrites `config`'s runtime manifest, `chiral-runtime.json`, after `change`.
fn edit_runtime_manifest(config: &ModelConfig, change: impl FnOnce(&mut Value)) {
    let path = config.runtime.as_ref().unwrap().runtime_dir.join("chiral-runtime.json");
    let mut manifest = read_json(&path).unwrap();
    change(&mut manifest);
    std::fs::write(&path, protocol::canonical_json(&manifest).unwrap()).unwrap();
}

/// Rewrites `config`'s artifact inventory, `chiral-artifact-sha256.json`, after `change`.
fn edit_artifact_manifest(config: &ModelConfig, change: impl FnOnce(&mut Value)) {
    let path = config.runtime.as_ref().unwrap().artifact_dir.join("chiral-artifact-sha256.json");
    let mut manifest = read_json(&path).unwrap();
    change(&mut manifest);
    std::fs::write(&path, protocol::canonical_json(&manifest).unwrap()).unwrap();
}

/// An inventory entry that binds exactly `bytes`.
fn inventory_entry(bytes: &[u8]) -> Value { json!({"sha256": hex::encode(Sha256::digest(bytes)), "size_bytes": bytes.len()}) }

/// The engine of `fixture` on a CUDA runtime: the manifest backend is `cuda` and `devices` are
/// selected with a 256 MiB reserve.
fn cuda_fixture(devices: &[&str]) -> (tempfile::TempDir, ModelConfig) {
    let (directory, mut config) = fixture();
    edit_runtime_manifest(&config, |manifest| manifest["backend"] = json!("cuda"));
    accelerated(&mut config, "cuda", devices, 256);
    (directory, config)
}

const RTX_4060: &str = "NVIDIA GeForce RTX 4060";

/// A serving profile that signs `config`'s verified runtime and artifact, its settings and, for
/// each selected device in order, an RTX 4060 with 8,188 MiB and driver `550.54.14`.
fn profile_of(config: &ModelConfig) -> crate::protocol::ServingProfile {
    let mut unprofiled = config.clone();
    unprofiled.model.serving = None;
    let validated = validate_inventory(&unprofiled).unwrap();
    let runtime = config.runtime.as_ref().unwrap();
    let mut profile = crate::performance::test_profile();
    (profile.hf_repo, profile.tokenizer_repo) = ("test/repo".into(), "test/repo".into());
    (profile.engine, profile.engine_version) = ("llama.cpp".into(), ENGINE_VERSION.into());
    (profile.runtime_sha256, profile.artifact_manifest_sha256) = (validated.runtime_sha256, validated.artifact_manifest_sha256);
    (profile.platform, profile.backend) = (platform().into(), runtime.backend.clone());
    profile.memory_kind = match runtime.backend.as_str() { "cpu" => "system", "metal" => "unified", _ => "dedicated" }.into();
    profile.devices = runtime.device_ids.iter().enumerate().map(|(index, selector)| EngineDevice {
        id: format!("GPU-0000000{index}-aaaa-bbbb-cccc-dddddddddddd"), name: RTX_4060.into(), memory_mib: 8188,
        driver_version: "550.54.14".into(), engine_device: selector.clone() }).collect();
    profile.memory_reserve_mib = runtime.memory_reserve_mib;
    (profile.gpu_layers, profile.split_mode, profile.tensor_split) = (runtime.gpu_layers, runtime.split_mode.clone(), runtime.tensor_split.clone());
    (profile.cache_type_k, profile.cache_type_v, profile.flash_attention) = (runtime.cache_type_k.clone(), runtime.cache_type_v.clone(), runtime.flash_attention);
    (profile.batch_size, profile.ubatch_size, profile.max_concurrency) = (runtime.batch_size, runtime.ubatch_size, config.max_concurrency);
    profile
}

/// A loopback port with no listener when this returns.
fn unused_port() -> u16 { std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port() }

/// Whether `pid` is gone (reaped) within five seconds.
#[cfg(unix)]
async fn gone_within_five_seconds(pid: u32) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while unsafe { libc::kill(pid as i32, 0) } == 0 && tokio::time::Instant::now() < deadline { tokio::time::sleep(Duration::from_millis(20)).await; }
    unsafe { libc::kill(pid as i32, 0) == -1 }
}

/// Waits, at most five seconds, until `engine`'s child has exited; the launcher sees the exit
/// from then on (tokio keeps the exit status).
fn wait_for_exit(engine: &ManagedEngine) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while engine.child.lock().unwrap().try_wait().unwrap().is_none() {
        assert!(std::time::Instant::now() < deadline, "the helper child did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// managed-engine-runtime task 3.1: a managed deployment runs only the pinned llama-server as a
// private Rust-supervised child (requirement 1).

const IDENTITY: &str = "runtime platform, backend, or pinned upstream identity mismatch";
const ENTRY_POINT: &str = "runtime entry point must be native llama-server";
const NOT_NATIVE: &str = "runtime entry point is not a native executable for this platform";

/// A runtime manifest of another engine release, source revision, schema, platform,
/// architecture or backend than the pin, this host and the configuration is refused by
/// inventory verification, the first step of `ManagedEngine::start`.
#[tokio::test]
async fn a_runtime_of_another_release_platform_architecture_or_backend_is_refused() {
    let (_directory, mut config) = fixture();
    let manifest_path = config.runtime.as_ref().unwrap().runtime_dir.join("chiral-runtime.json");
    let original = std::fs::read(&manifest_path).unwrap();
    let mut cases: Vec<(String, &str, Value)> = vec![
        ("engine version v0.8.0".into(), "engine_version", json!("v0.8.0")),
        ("engine version 0.9.0".into(), "engine_version", json!("0.9.0")),
        ("engine version v0.9.1".into(), "engine_version", json!("v0.9.1")),
        ("source revision master".into(), "source_revision", json!("master")),
        ("an abbreviated source revision".into(), "source_revision", json!(&SOURCE_REVISION[..7])),
        ("an upper-case source revision".into(), "source_revision", json!(SOURCE_REVISION.to_uppercase())),
        ("schema v2".into(), "schema", json!("chiral-llama-runtime-v2")),
        ("platform darwin".into(), "platform", json!("darwin")),
        ("architecture arm64".into(), "architecture", json!("arm64")),
        ("architecture amd64".into(), "architecture", json!("amd64")),
        ("backend cuda".into(), "backend", json!("cuda")),
        ("backend metal".into(), "backend", json!("metal")),
        ("backend vulkan".into(), "backend", json!("vulkan")),
    ];
    for other in ["linux", "macos", "windows"].into_iter().filter(|os| *os != platform()) {
        cases.push((format!("platform {other}"), "platform", json!(other)));
    }
    for other in ["x86_64", "aarch64"].into_iter().filter(|arch| *arch != std::env::consts::ARCH) {
        cases.push((format!("architecture {other}"), "architecture", json!(other)));
    }
    for (name, field, value) in cases {
        edit_runtime_manifest(&config, |manifest| manifest[field] = value);
        assert_eq!(validate_inventory(&config).err().as_deref(), Some(IDENTITY), "{name}");
        assert_eq!(ManagedEngine::start(&config).await.err().as_deref(), Some(IDENTITY), "{name}: ManagedEngine::start");
        std::fs::write(&manifest_path, &original).unwrap();
    }
    assert!(validate_inventory(&config).is_ok());
    // The configured backend differs from the runtime's: a CUDA configuration of a CPU runtime.
    accelerated(&mut config, "cuda", &["CUDA0"], 256);
    assert_eq!(validate_inventory(&config).err().as_deref(), Some(IDENTITY), "a CUDA configuration of a CPU runtime");
    edit_runtime_manifest(&config, |manifest| manifest["backend"] = json!("cuda"));
    assert!(validate_inventory(&config).is_ok(), "a CUDA configuration of a CUDA runtime");
}

/// Replaces `config`'s runtime entry point with an inventoried file `name` holding `bytes`
/// (executable on Unix).
fn install_entry_point(config: &ModelConfig, name: &str, bytes: &[u8]) {
    let runtime = &config.runtime.as_ref().unwrap().runtime_dir;
    let previous = read_json(&runtime.join("chiral-runtime.json")).unwrap()["executable"].as_str().unwrap().to_owned();
    std::fs::remove_file(runtime.join(&previous)).unwrap();
    std::fs::write(runtime.join(name), bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(runtime.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    edit_runtime_manifest(config, |manifest| {
        manifest["executable"] = json!(name);
        manifest["files"] = json!({name: inventory_entry(bytes)});
    });
}

/// The first `len` bytes of an executable header: `prefix`, then zeros.
fn header(prefix: &[u8], len: usize) -> Vec<u8> { let mut bytes = prefix.to_vec(); bytes.resize(len, 0); bytes }

/// Only an inventoried `llama-server` with a native header for this host is accepted. An
/// entry point named `run.sh`, a `llama-server` that is a shell script padded to 4,096 bytes,
/// and native headers of other platforms and architectures are refused by inventory
/// verification, and the file is never executed: the scripts would create a marker file.
#[tokio::test]
async fn only_an_inventoried_native_llama_server_entry_point_is_accepted_and_nothing_else_runs() {
    let (directory, mut config) = fixture();
    config.engine_url = format!("http://127.0.0.1:{}", unused_port());
    let engine_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    let native = std::fs::read(config.runtime.as_ref().unwrap().runtime_dir.join(engine_name)).unwrap();
    let marker = directory.path().join("entry-point-executed");
    // `#!/bin/sh\n`, a command that leaves the marker, padded with newlines to 4,096 bytes.
    let script = header(format!("#!/bin/sh\necho executed > '{}'\n", marker.display()).as_bytes(), 4096)
        .into_iter().map(|byte| if byte == 0 { b'\n' } else { byte }).collect::<Vec<_>>();
    assert!(script.starts_with(b"#!/bin/sh\n") && script.len() == 4096);
    let macho = |cpu: u32| header(&[b"\xcf\xfa\xed\xfe".as_slice(), &cpu.to_le_bytes()].concat(), 4096);
    let elf = |machine: u16| { let mut bytes = header(b"\x7fELF\x02\x01", 4096); bytes[18..20].copy_from_slice(&machine.to_le_bytes()); bytes };
    let pe = |machine: u16| {
        let mut bytes = header(b"MZ", 4096);
        bytes[60..64].copy_from_slice(&128u32.to_le_bytes());
        bytes[128..132].copy_from_slice(b"PE\0\0");
        bytes[132..134].copy_from_slice(&machine.to_le_bytes());
        bytes
    };
    let other_name = if cfg!(windows) { "llama-server" } else { "llama-server.exe" };
    let refused: Vec<(&str, &str, Vec<u8>, &str)> = vec![
        ("run.sh with this host's native header", "run.sh", native.clone(), ENTRY_POINT),
        ("run.sh, a shell script", "run.sh", script.clone(), ENTRY_POINT),
        ("the other platforms' engine name", other_name, native.clone(), ENTRY_POINT),
        ("llama-server-wrapper", "llama-server-wrapper", native.clone(), ENTRY_POINT),
        ("llama-server, a shell script of 4,096 bytes", engine_name, script.clone(), NOT_NATIVE),
        ("an empty llama-server", engine_name, Vec::new(), NOT_NATIVE),
    ];
    for (name, file, bytes, error) in refused {
        install_entry_point(&config, file, &bytes);
        assert_eq!(validate_inventory(&config).err().as_deref(), Some(error), "{name}");
        assert_eq!(ManagedEngine::start(&config).await.err().as_deref(), Some(error), "{name}: ManagedEngine::start");
        assert!(!marker.exists(), "{name}: the entry point was executed");
    }
    // Native headers: accepted only for this host's platform and architecture.
    let headers = [("Mach-O arm64", "macos", "aarch64", macho(0x0100_000c)), ("Mach-O x86_64", "macos", "x86_64", macho(0x0100_0007)),
        ("ELF aarch64", "linux", "aarch64", elf(183)), ("ELF x86-64", "linux", "x86_64", elf(62)),
        ("PE arm64", "windows", "aarch64", pe(0xaa64)), ("PE x86-64", "windows", "x86_64", pe(0x8664)),
        ("32-bit ELF", "none", "none", header(b"\x7fELF\x01\x01", 4096)), ("32-bit Mach-O", "none", "none", header(b"\xce\xfa\xed\xfe\x0c\x00\x00\x00", 4096))];
    let mut accepted = 0;
    for (name, os, arch, bytes) in headers {
        install_entry_point(&config, engine_name, &bytes);
        if os == platform() && arch == std::env::consts::ARCH {
            assert!(validate_inventory(&config).is_ok(), "{name} on this host");
            accepted += 1;
        } else {
            assert_eq!(validate_inventory(&config).err().as_deref(), Some(NOT_NATIVE), "{name}");
        }
    }
    assert_eq!(accepted, usize::from(["macos", "linux", "windows"].contains(&platform()) && ["aarch64", "x86_64"].contains(&std::env::consts::ARCH)));
    // The marker detects execution: running the script directly leaves it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let control = directory.path().join("control.sh");
        std::fs::write(&control, &script).unwrap();
        std::fs::set_permissions(&control, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(std::process::Command::new(&control).status().unwrap().success());
        assert!(marker.exists(), "the control script left no marker");
    }
}

// managed-engine-runtime task 3.2: runtime and artifact inventories are verified before every
// launch (requirement 2).

const MISSING: &str = "immutable inventory file is missing";
const SIZE: &str = "inventory file size mismatch";
const DIGEST: &str = "inventory file digest mismatch";
const UNEXPECTED: &str = "unexpected file in immutable inventory";
const ENTRY_GGUF: &str = "GGUF entry point is not inventoried";
const ALIAS: &str = "model API alias differs from selected GGUF identity";
const PROFILE: &str = "managed launch settings differ from immutable serving terms";

type InventoryCase = (&'static str, Box<dyn Fn(&mut ModelConfig)>, &'static str);

fn inventory_case(name: &'static str, change: impl Fn(&mut ModelConfig) + 'static, error: &'static str) -> InventoryCase {
    (name, Box::new(change), error)
}

fn runtime_dir(config: &ModelConfig) -> PathBuf { config.runtime.as_ref().unwrap().runtime_dir.clone() }
fn artifact_dir(config: &ModelConfig) -> PathBuf { config.runtime.as_ref().unwrap().artifact_dir.clone() }

/// Writes `config.model`, with model identifier `id`, as the artifact's ModelSpec too.
fn rename_model(config: &mut ModelConfig, id: &str) {
    config.model.id = id.into();
    std::fs::write(artifact_dir(config).join("chiral-model.json"), protocol::canonical_json(&config.model).unwrap()).unwrap();
}

/// A removed, resized or uninventoried file, and a model identifier other than
/// `<repository>/<GGUF file name>`, are each refused, with the same error from
/// `ManagedEngine::start`.
#[tokio::test]
async fn removed_resized_or_uninventoried_files_and_another_model_identifier_are_refused() {
    let engine_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    let gguf = |config: &ModelConfig| artifact_dir(config).join("model-Q4_K_M.gguf");
    let cases = vec![
        // An inventoried file removed.
        inventory_case("the runtime executable removed", move |c| std::fs::remove_file(runtime_dir(c).join(engine_name)).unwrap(), MISSING),
        inventory_case("the GGUF removed", move |c| std::fs::remove_file(gguf(c)).unwrap(), MISSING),
        inventory_case("the template removed", |c| std::fs::remove_file(artifact_dir(c).join("chiral-chat-template.jinja")).unwrap(), MISSING),
        inventory_case("the artifact ModelSpec removed", |c| std::fs::remove_file(artifact_dir(c).join("chiral-model.json")).unwrap(), MISSING),
        // A file whose size differs from its entry: bytes added or removed, or the entry's size
        // changed while the bytes, and so their digest, are unchanged.
        inventory_case("a byte appended to the GGUF", move |c| { let mut bytes = std::fs::read(gguf(c)).unwrap(); bytes.push(b'X'); std::fs::write(gguf(c), bytes).unwrap(); }, SIZE),
        inventory_case("the GGUF truncated by a byte", move |c| { let bytes = std::fs::read(gguf(c)).unwrap(); std::fs::write(gguf(c), &bytes[..bytes.len() - 1]).unwrap(); }, SIZE),
        inventory_case("the GGUF entry one byte larger", |c| edit_artifact_manifest(c, |m| { let size = m["files"]["model-Q4_K_M.gguf"]["size_bytes"].as_u64().unwrap(); m["files"]["model-Q4_K_M.gguf"]["size_bytes"] = json!(size + 1); }), SIZE),
        inventory_case("the executable entry one byte smaller", move |c| edit_runtime_manifest(c, |m| { let size = m["files"][engine_name]["size_bytes"].as_u64().unwrap(); m["files"][engine_name]["size_bytes"] = json!(size - 1); }), SIZE),
        // A changed byte and an unlisted file (the existing regressions, here with their errors).
        inventory_case("one GGUF byte changed", move |c| { let mut bytes = std::fs::read(gguf(c)).unwrap(); bytes[0] ^= 1; std::fs::write(gguf(c), bytes).unwrap(); }, DIGEST),
        inventory_case("an unlisted library", |c| std::fs::write(runtime_dir(c).join("unlisted.dll"), b"untrusted loader input").unwrap(), UNEXPECTED),
        inventory_case("an unlisted GGUF", |c| std::fs::write(artifact_dir(c).join("model-Q8_0.gguf"), b"GGUF").unwrap(), UNEXPECTED),
        // A `model_file` that the inventory does not list, or that is not a GGUF.
        inventory_case("model_file not in the inventory", |c| edit_artifact_manifest(c, |m| m["model_file"] = json!("model-Q5_K_M.gguf")), ENTRY_GGUF),
        inventory_case("model_file in an uninventoried directory", |c| edit_artifact_manifest(c, |m| m["model_file"] = json!("sub/model-Q4_K_M.gguf")), ENTRY_GGUF),
        inventory_case("model_file naming the template", |c| edit_artifact_manifest(c, |m| m["model_file"] = json!("chiral-chat-template.jinja")), ENTRY_GGUF),
        // A model API identifier other than `test/repo/model-Q4_K_M.gguf`, in the configuration
        // and the artifact ModelSpec alike.
        inventory_case("identifier without the .gguf suffix", |c| rename_model(c, "test/repo/model-Q4_K_M"), ALIAS),
        inventory_case("identifier with another letter case", |c| rename_model(c, "test/repo/model-q4_k_m.gguf"), ALIAS),
        inventory_case("identifier of another repository", |c| rename_model(c, "other/repo/model-Q4_K_M.gguf"), ALIAS),
        inventory_case("identifier without the repository", |c| rename_model(c, "model-Q4_K_M.gguf"), ALIAS),
        inventory_case("identifier of another file", |c| rename_model(c, "test/repo/model-Q8_0.gguf"), ALIAS),
        inventory_case("the artifact of another repository", |c| edit_artifact_manifest(c, |m| m["model"]["repo_id"] = json!("test/other")), ALIAS),
    ];
    for (name, change, error) in cases {
        let (_directory, mut config) = fixture();
        assert!(validate_inventory(&config).is_ok(), "{name}: the unaltered fixture");
        change(&mut config);
        assert_eq!(validate_inventory(&config).err().as_deref(), Some(error), "{name}");
        assert_eq!(ManagedEngine::start(&config).await.err().as_deref(), Some(error), "{name}: ManagedEngine::start");
    }
}

/// A serving profile must equal the verified runtime and artifact and every launch setting:
/// each difference is refused, and the matching profile is accepted.
#[test]
fn a_serving_profile_must_equal_every_verified_identity_and_launch_setting() {
    let (_directory, mut config) = cuda_fixture(&["CUDA0", "CUDA1"]);
    let runtime = runtime_of(&mut config);
    (runtime.tensor_split, runtime.flash_attention, runtime.cache_type_k, runtime.cache_type_v) = (vec![3, 1], true, "q8_0".into(), "q4_0".into());
    (runtime.batch_size, runtime.ubatch_size) = (512, 256);
    let profile = profile_of(&config);
    config.model.serving = Some(profile.clone());
    assert!(validate_inventory(&config).is_ok(), "the matching profile");
    let other_platform = if platform() == "linux" { "macos" } else { "linux" };
    let cases: Vec<(&str, Box<dyn Fn(&mut crate::protocol::ServingProfile)>)> = vec![
        ("memory_reserve_mib", Box::new(|p| p.memory_reserve_mib = 512)),
        ("gpu_layers", Box::new(|p| p.gpu_layers = 998)),
        ("split_mode", Box::new(|p| p.split_mode = "row".into())),
        ("tensor_split", Box::new(|p| p.tensor_split = vec![1, 3])),
        ("cache_type_k", Box::new(|p| p.cache_type_k = "f16".into())),
        ("cache_type_v", Box::new(|p| p.cache_type_v = "q8_0".into())),
        ("flash_attention", Box::new(|p| p.flash_attention = false)),
        ("batch_size", Box::new(|p| p.batch_size = 1024)),
        ("ubatch_size", Box::new(|p| p.ubatch_size = 128)),
        ("max_concurrency", Box::new(|p| p.max_concurrency = 3)),
        ("device selectors in another order", Box::new(|p| p.devices.swap(0, 1))),
        ("one device selector fewer", Box::new(|p| { p.devices.pop(); })),
        ("another device selector", Box::new(|p| p.devices[1].engine_device = "CUDA2".into())),
        ("runtime_sha256", Box::new(|p| p.runtime_sha256 = "00".repeat(32))),
        ("artifact_manifest_sha256", Box::new(|p| p.artifact_manifest_sha256 = "00".repeat(32))),
        ("engine", Box::new(|p| p.engine = "vllm".into())),
        ("engine_version", Box::new(|p| p.engine_version = "v0.8.0".into())),
        ("hf_repo", Box::new(|p| p.hf_repo = "other/repo".into())),
        ("tokenizer_repo", Box::new(|p| p.tokenizer_repo = "other/repo".into())),
        ("platform", Box::new(move |p| p.platform = other_platform.into())),
        ("backend", Box::new(|p| p.backend = "vulkan".into())),
    ];
    for (name, change) in cases {
        let mut differing = profile.clone();
        change(&mut differing);
        config.model.serving = Some(differing);
        assert_eq!(validate_inventory(&config).err().as_deref(), Some(PROFILE), "{name}");
    }
    // The scenario: the profile states batch size 512 and the runtime configuration 256.
    config.model.serving = Some(profile);
    runtime_of(&mut config).batch_size = 256;
    assert_eq!(validate_inventory(&config).err().as_deref(), Some(PROFILE), "runtime batch size 256 under a profile of 512");
}

/// Two runtime packages of the same upstream commit whose inventories differ only in one
/// library's digest have different runtime identities, the SHA-256 of the canonical manifest,
/// and a profile that signs one is refused when the other is installed.
#[test]
fn a_repackaged_runtime_has_another_identity() {
    let library = if cfg!(windows) { "ggml-base.dll" } else if cfg!(target_os = "macos") { "libggml-base.dylib" } else { "libggml-base.so" };
    let package = |bytes: &'static [u8]| {
        let (directory, config) = fixture();
        std::fs::write(runtime_dir(&config).join(library), bytes).unwrap();
        edit_runtime_manifest(&config, |manifest| manifest["files"][library] = inventory_entry(bytes));
        (directory, config)
    };
    let (_first_directory, first) = package(b"ggml base library, build A");
    let (_second_directory, second) = package(b"ggml base library, build B");
    let manifests = [&first, &second].map(|config| read_json(&runtime_dir(config).join("chiral-runtime.json")).unwrap());
    // Only that digest differs.
    let mut aligned = manifests[0].clone();
    assert_ne!(aligned, manifests[1]);
    aligned["files"][library]["sha256"] = manifests[1]["files"][library]["sha256"].clone();
    assert_eq!(aligned, manifests[1]);
    let (one, other) = (validate_inventory(&first).unwrap(), validate_inventory(&second).unwrap());
    assert_ne!(one.runtime_sha256, other.runtime_sha256);
    assert_eq!(one.artifact_manifest_sha256, other.artifact_manifest_sha256);
    for (validated, manifest) in [(&one, &manifests[0]), (&other, &manifests[1])] {
        assert_eq!(validated.runtime_sha256, hex::encode(Sha256::digest(protocol::canonical_json(manifest).unwrap())));
    }
    let signed_first = profile_of(&first);
    let mut installed_second = second.clone();
    installed_second.model.serving = Some(signed_first.clone());
    assert_eq!(validate_inventory(&installed_second).err().as_deref(), Some(PROFILE));
    let mut installed_first = first.clone();
    installed_first.model.serving = Some(signed_first);
    assert!(validate_inventory(&installed_first).is_ok());
}

/// `ManagedEngine::start` on an altered inventory returns the inventory error before it touches
/// the endpoint: the port stays unbound, and a listener there is neither reported as occupying it
/// nor disturbed. The unaltered fixture reaches the port check and the spawn.
#[tokio::test]
async fn an_altered_inventory_refuses_the_launch_before_the_port_or_any_process() {
    let engine_name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    let alterations = vec![
        inventory_case("an unlisted library", |c| std::fs::write(runtime_dir(c).join("unlisted.dll"), b"untrusted loader input").unwrap(), UNEXPECTED),
        inventory_case("one GGUF byte changed", |c| { let path = artifact_dir(c).join("model-Q4_K_M.gguf"); let mut bytes = std::fs::read(&path).unwrap(); bytes[0] ^= 1; std::fs::write(path, bytes).unwrap(); }, DIGEST),
        inventory_case("the template removed", |c| std::fs::remove_file(artifact_dir(c).join("chiral-chat-template.jinja")).unwrap(), MISSING),
        inventory_case("the executable's digest", move |c| edit_runtime_manifest(c, |m| m["files"][engine_name]["sha256"] = json!("00".repeat(32))), DIGEST),
        inventory_case("source revision master", |c| edit_runtime_manifest(c, |m| m["source_revision"] = json!("master")), IDENTITY),
    ];
    for (name, change, error) in alterations {
        let (_directory, mut config) = fixture();
        let port = unused_port();
        config.engine_url = format!("http://127.0.0.1:{port}");
        change(&mut config);
        assert_eq!(ManagedEngine::start(&config).await.err().as_deref(), Some(error), "{name}");
        // Nothing listens on the endpoint, and the launcher holds nothing there.
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(), "{name}: the port was left bound");
        let listener = foreign_listener(format!("127.0.0.1:{port}").parse().unwrap(), false).await;
        assert_eq!(ManagedEngine::start(&config).await.err().as_deref(), Some(error), "{name}: with a listener on the port");
        assert_still_accepts(&listener, format!("127.0.0.1:{port}").parse().unwrap()).await;
        // On a CUDA runtime, no device observer runs either, the engine's enumeration included.
        let (_directory, mut cuda) = cuda_fixture(&["CUDA0"]);
        cuda.engine_url = config.engine_url.clone();
        change(&mut cuda);
        let recorded = Recorded::new(rtx_4060(7900, "550.54.14"));
        assert_eq!(ManagedEngine::start_observed(&cuda, &recorded).await.err().as_deref(), Some(error), "{name}: CUDA");
        assert!(recorded.runs().is_empty(), "{name}: a device observer ran: {:?}", recorded.runs());
    }
    // The control: unaltered, the same launch reaches the port check, and then the spawn.
    let (_directory, mut config) = fixture();
    let listener = foreign_listener("127.0.0.1:0".parse().unwrap(), false).await;
    config.engine_url = format!("http://{}", listener.local_addr().unwrap());
    assert_eq!(ManagedEngine::start(&config).await.err().as_deref(), Some(OCCUPIED_ENGINE_PORT));
    drop(listener);
    config.engine_url = format!("http://127.0.0.1:{}", unused_port());
    let error = ManagedEngine::start(&config).await.err().unwrap();
    assert!(failed_after_port_check(&error), "{error}");
    // Unaltered on CUDA, the observers run before the port check and the spawn.
    let (_directory, mut cuda) = cuda_fixture(&["CUDA0"]);
    cuda.engine_url = format!("http://127.0.0.1:{}", unused_port());
    let recorded = Recorded::new(rtx_4060(7900, "550.54.14"));
    let error = ManagedEngine::start_observed(&cuda, &recorded).await.err().unwrap();
    assert!(failed_after_port_check(&error), "{error}");
    assert_eq!(recorded.runs(), [LIST_DEVICES, NVIDIA_SMI]);
}

/// An inventoried library in the runtime's subdirectory `lib`.
const NESTED_LIBRARY: &str = if cfg!(windows) { "lib/ggml-base.dll" } else if cfg!(target_os = "macos") { "lib/libggml-base.dylib" } else { "lib/libggml-base.so" };

/// Adds `NESTED_LIBRARY` to `config`'s runtime and its inventory.
fn add_nested_library(config: &ModelConfig) {
    let bytes = b"nested ggml base library";
    std::fs::create_dir(runtime_dir(config).join("lib")).unwrap();
    std::fs::write(runtime_dir(config).join(NESTED_LIBRARY), bytes).unwrap();
    edit_runtime_manifest(config, |manifest| manifest["files"][NESTED_LIBRARY] = inventory_entry(bytes));
}

/// Pads the JSON file at `path` with trailing newlines, which JSON ignores, to `len` bytes.
fn pad_to(path: &Path, len: usize) {
    let mut bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() <= len);
    bytes.resize(len, b'\n');
    std::fs::write(path, bytes).unwrap();
}

/// Moves `path` to `outside` and leaves a symbolic link to it, so that the bytes reached through
/// `path` are unchanged.
#[cfg(unix)]
fn link_to_moved(path: &Path, outside: &Path) {
    std::fs::rename(path, outside).unwrap();
    std::os::unix::fs::symlink(outside, path).unwrap();
}

type TreeCase = (&'static str, Box<dyn Fn(&ModelConfig, &Path)>, &'static str);

fn tree_case(name: &'static str, change: impl Fn(&ModelConfig, &Path) + 'static, error: &'static str) -> TreeCase {
    (name, Box::new(change), error)
}

/// Task 3.2 (requirement 2), found missing by adversarial review: every level of an inventory is
/// verified. A file in a subdirectory is bound like a top-level one, and an unlisted file in any
/// subdirectory is refused. A symbolic link is refused wherever it is, even when the bytes it
/// reaches are the inventoried ones: an inventoried file, a subdirectory, the inventory root or
/// a manifest. Each manifest is a regular file of at most 4 MiB, and each inventory path a safe,
/// unreserved relative path with a lower-case SHA-256. `ManagedEngine::start` returns the same
/// error, and on this CUDA runtime no device observer runs.
#[tokio::test]
async fn every_level_of_an_inventory_is_verified_and_no_symbolic_link_or_unbounded_manifest_is_followed() {
    const SYMLINK: &str = "inventory symlinks are forbidden";
    const ROOT: &str = "invalid inventory directory or file count";
    const METADATA: &str = "runtime metadata must be a bounded regular file";
    const UNSAFE: &str = "inventory path is not a safe relative path";
    const ENTRY: &str = "invalid inventory digest or reserved filename";
    const BOUND: usize = 4 * 1024 * 1024;
    let nested = |c: &ModelConfig| runtime_dir(c).join(NESTED_LIBRARY);
    #[allow(unused_mut)]
    let mut cases = vec![
        tree_case("a nested library byte changed", move |c, _| { let mut bytes = std::fs::read(nested(c)).unwrap(); bytes[0] ^= 1; std::fs::write(nested(c), bytes).unwrap(); }, DIGEST),
        tree_case("a nested library removed", move |c, _| std::fs::remove_file(nested(c)).unwrap(), MISSING),
        tree_case("an unlisted library beside the nested one", |c, _| std::fs::write(runtime_dir(c).join("lib/unlisted.so"), b"untrusted loader input").unwrap(), UNEXPECTED),
        tree_case("an unlisted library in a new runtime subdirectory", |c, _| { std::fs::create_dir_all(runtime_dir(c).join("backends/cuda")).unwrap(); std::fs::write(runtime_dir(c).join("backends/cuda/libggml-cuda.so"), b"untrusted backend").unwrap(); }, UNEXPECTED),
        tree_case("an unlisted GGUF in a new artifact subdirectory", |c, _| { std::fs::create_dir(artifact_dir(c).join("extra")).unwrap(); std::fs::write(artifact_dir(c).join("extra/model-Q8_0.gguf"), b"GGUF").unwrap(); }, UNEXPECTED),
        tree_case("the runtime manifest one byte above 4 MiB", |c, _| pad_to(&runtime_dir(c).join("chiral-runtime.json"), BOUND + 1), METADATA),
        tree_case("the artifact manifest one byte above 4 MiB", |c, _| pad_to(&artifact_dir(c).join("chiral-artifact-sha256.json"), BOUND + 1), METADATA),
        tree_case("the artifact ModelSpec one byte above 4 MiB", |c, _| {
            // Inventory verification passes, so the ModelSpec read is what refuses it.
            pad_to(&artifact_dir(c).join("chiral-model.json"), BOUND + 1);
        }, METADATA),
        tree_case("the runtime manifest a directory", |c, _| { std::fs::remove_file(runtime_dir(c).join("chiral-runtime.json")).unwrap(); std::fs::create_dir(runtime_dir(c).join("chiral-runtime.json")).unwrap(); }, METADATA),
        tree_case("an inventory path through ..", |c, _| edit_runtime_manifest(c, |m| m["files"]["lib/../llama-server"] = inventory_entry(b"x")), UNSAFE),
        tree_case("an absolute inventory path", |c, _| edit_runtime_manifest(c, |m| m["files"]["/etc/hosts"] = inventory_entry(b"x")), UNSAFE),
        tree_case("a backslash inventory path", |c, _| edit_runtime_manifest(c, |m| m["files"]["lib\\libggml-base.so"] = inventory_entry(b"x")), UNSAFE),
        tree_case("an inventory path with a drive or stream colon", |c, _| edit_runtime_manifest(c, |m| m["files"]["C:llama-server"] = inventory_entry(b"x")), UNSAFE),
        tree_case("an inventory entry naming the runtime manifest", |c, _| edit_runtime_manifest(c, |m| m["files"]["chiral-runtime.json"] = inventory_entry(b"x")), ENTRY),
        tree_case("an inventory entry naming the artifact ModelSpec", |c, _| edit_artifact_manifest(c, |m| m["files"]["chiral-model.json"] = inventory_entry(b"x")), ENTRY),
        tree_case("an upper-case digest", |c, _| edit_runtime_manifest(c, |m| { let digest = m["files"][NESTED_LIBRARY]["sha256"].as_str().unwrap().to_uppercase(); m["files"][NESTED_LIBRARY]["sha256"] = json!(digest); }), ENTRY),
        tree_case("a 63-character digest", |c, _| edit_runtime_manifest(c, |m| { let digest = m["files"][NESTED_LIBRARY]["sha256"].as_str().unwrap()[1..].to_owned(); m["files"][NESTED_LIBRARY]["sha256"] = json!(digest); }), ENTRY),
        tree_case("an empty runtime inventory", |c, _| edit_runtime_manifest(c, |m| m["files"] = json!({})), ROOT),
    ];
    #[cfg(unix)]
    cases.extend([
        tree_case("the inventoried GGUF a link to its own bytes outside the artifact", |c, outside| link_to_moved(&artifact_dir(c).join("model-Q4_K_M.gguf"), &outside.join("model-Q4_K_M.gguf")), SYMLINK),
        tree_case("the runtime executable a link to its own bytes outside the runtime", |c, outside| link_to_moved(&runtime_dir(c).join("llama-server"), &outside.join("llama-server")), SYMLINK),
        tree_case("the nested library a link to its own bytes outside the runtime", move |c, outside| link_to_moved(&nested(c), &outside.join("library")), SYMLINK),
        tree_case("the library directory a link to itself moved outside the runtime", |c, outside| link_to_moved(&runtime_dir(c).join("lib"), &outside.join("lib")), SYMLINK),
        tree_case("alias.gguf, a link to the inventoried GGUF", |c, _| std::os::unix::fs::symlink("model-Q4_K_M.gguf", artifact_dir(c).join("alias.gguf")).unwrap(), SYMLINK),
        tree_case("the artifact ModelSpec a link to its own bytes outside the artifact", |c, outside| link_to_moved(&artifact_dir(c).join("chiral-model.json"), &outside.join("chiral-model.json")), SYMLINK),
        tree_case("the runtime manifest a link to its own bytes outside the runtime", |c, outside| link_to_moved(&runtime_dir(c).join("chiral-runtime.json"), &outside.join("chiral-runtime.json")), METADATA),
        tree_case("the artifact manifest a link to its own bytes outside the artifact", |c, outside| link_to_moved(&artifact_dir(c).join("chiral-artifact-sha256.json"), &outside.join("chiral-artifact-sha256.json")), METADATA),
        tree_case("the runtime directory a link to itself moved elsewhere", |c, outside| link_to_moved(&runtime_dir(c), &outside.join("runtime")), ROOT),
        tree_case("the artifact directory a link to itself moved elsewhere", |c, outside| link_to_moved(&artifact_dir(c), &outside.join("artifact")), ROOT),
    ]);
    let fresh = || {
        let (directory, mut config) = cuda_fixture(&["CUDA0"]);
        add_nested_library(&config);
        config.engine_url = format!("http://127.0.0.1:{}", unused_port());
        let outside = directory.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        (directory, config, outside)
    };
    for (name, change, error) in cases {
        let (_directory, config, outside) = fresh();
        assert!(validate_inventory(&config).is_ok(), "{name}: the unaltered fixture");
        change(&config, &outside);
        assert_eq!(validate_inventory(&config).err().as_deref(), Some(error), "{name}");
        let recorded = Recorded::new(rtx_4060(7900, "550.54.14"));
        assert_eq!(ManagedEngine::start_observed(&config, &recorded).await.err().as_deref(), Some(error), "{name}: ManagedEngine::start");
        assert!(recorded.runs().is_empty(), "{name}: a device observer ran: {:?}", recorded.runs());
    }
    // Accepted: manifests of exactly 4 MiB, the bound itself.
    let (_directory, config, _outside) = fresh();
    pad_to(&runtime_dir(&config).join("chiral-runtime.json"), BOUND);
    pad_to(&artifact_dir(&config).join("chiral-artifact-sha256.json"), BOUND);
    pad_to(&artifact_dir(&config).join("chiral-model.json"), BOUND);
    assert!(validate_inventory(&config).is_ok(), "manifests of exactly 4 MiB");
    // The control: with the nested library, the launch observes both devices and reaches the spawn.
    let recorded = Recorded::new(rtx_4060(7900, "550.54.14"));
    let error = ManagedEngine::start_observed(&config, &recorded).await.err().unwrap();
    assert!(failed_after_port_check(&error), "{error}");
    assert_eq!(recorded.runs(), [LIST_DEVICES, NVIDIA_SMI]);
}

// managed-engine-runtime task 3.4: the engine listens only on its literal-loopback endpoint, and
// an occupied port is refused (requirement 4).

const ENDPOINT: &str = "managed engine requires HTTP on an explicit literal loopback address and port";
const ORIGIN: &str = "endpoint must be an origin without credentials, path, query or fragment";
const PRIVATE_OR_LOOPBACK: &str = "engine endpoints require a literal private/loopback IP or localhost";

/// The managed endpoint is `http://` with a literal loopback IP and an explicit nonzero port,
/// without credentials, path, query or fragment: refused by `validate` and by configuration
/// loading otherwise. Configuration loading of a non-private address stops at the general engine
/// endpoint rule first.
#[test]
fn a_managed_endpoint_is_literal_loopback_with_an_explicit_nonzero_port() {
    let (directory, mut config) = fixture();
    let cases = [
        ("http://localhost:18080", ENDPOINT, ENDPOINT),
        ("http://192.168.1.2:18080", ENDPOINT, ENDPOINT),
        ("http://10.0.0.1:18080", ENDPOINT, ENDPOINT),
        ("http://[fd00::1]:18080", ENDPOINT, ENDPOINT),
        ("https://127.0.0.1:18080", ENDPOINT, ENDPOINT),
        ("http://127.0.0.1", ENDPOINT, ENDPOINT),
        ("http://127.0.0.1:0", ENDPOINT, ENDPOINT),
        ("http://[::1]", ENDPOINT, ENDPOINT),
        ("http://[::1]:0", ENDPOINT, ENDPOINT),
        ("http://0.0.0.0:18080", ENDPOINT, PRIVATE_OR_LOOPBACK),
        ("http://[::]:18080", ENDPOINT, PRIVATE_OR_LOOPBACK),
        ("http://engine:secret@127.0.0.1:18080", ORIGIN, ORIGIN),
        ("http://127.0.0.1:18080/v1", ORIGIN, ORIGIN),
        ("http://127.0.0.1:18080/?model=other", ORIGIN, ORIGIN),
        ("http://127.0.0.1:18080/#health", ORIGIN, ORIGIN),
    ];
    for (endpoint, validated, loaded) in cases {
        config.engine_url = endpoint.into();
        assert_eq!(config.runtime.as_ref().unwrap().validate(&config).err().as_deref(), Some(validated), "{endpoint}");
        assert_eq!(load_provider(directory.path(), &provider_json(&config)).err().as_deref(), Some(loaded), "{endpoint}: configuration loading");
    }
    for endpoint in ["http://127.0.0.1:18080", "http://127.0.0.1:18080/", "http://[::1]:18080", "http://127.0.0.1:65535"] {
        config.engine_url = endpoint.into();
        assert_eq!(config.runtime.as_ref().unwrap().validate(&config), Ok(()), "{endpoint}");
        let loaded = load_provider(directory.path(), &provider_json(&config)).unwrap_or_else(|error| panic!("{endpoint}: {error}"));
        assert_eq!(loaded.models[0].engine_url, endpoint);
    }
}

/// Whether a connection to `endpoint` completes within one second. A listening socket accepts
/// at once; a port with no listener refuses, and on macOS a port whose socket is bound without
/// listening drops the connection request, so the attempt does not complete.
async fn connects(endpoint: std::net::SocketAddr) -> bool {
    matches!(tokio::time::timeout(Duration::from_secs(1), tokio::net::TcpStream::connect(endpoint)).await, Ok(Ok(_)))
}

/// The wildcard half of the occupied-port check is bound but does not listen: no connection to
/// the port completes. The control: a wildcard socket that listens accepts one at once.
#[tokio::test]
async fn the_occupied_port_check_listens_on_no_wildcard_address() {
    let mut endpoints = vec![std::net::SocketAddr::from(([127, 0, 0, 1], unused_port()))];
    if let Ok(listener) = std::net::TcpListener::bind("[::1]:0") {
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        endpoints.push(std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, port)));
    }
    for endpoint in endpoints {
        let wildcard: std::net::IpAddr = if endpoint.is_ipv4() { std::net::Ipv4Addr::UNSPECIFIED.into() } else { std::net::Ipv6Addr::UNSPECIFIED.into() };
        let probe = wildcard_port_probe(endpoint).unwrap();
        assert_eq!(probe.local_addr().unwrap(), std::net::SocketAddr::new(wildcard, endpoint.port()));
        assert!(!connects(endpoint).await, "{endpoint}: the wildcard probe accepted a connection");
        drop(probe);
        let listening = foreign_listener(std::net::SocketAddr::new(wildcard, endpoint.port()), false).await;
        assert!(connects(endpoint).await, "{endpoint}: the control listener accepted no connection");
        drop(listening);
    }
}

/// Serves `GET /health` with 200, as a loaded engine does, after running `on_health`: the URL
/// and the server task.
async fn health_server(on_health: impl Fn() + Clone + Send + Sync + 'static) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route("/health", axum::routing::get(move || {
        let on_health = on_health.clone();
        async move { on_health(); axum::Json(json!({"status": "ok"})) }
    }));
    (url, tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); }))
}

/// Another process's `/health` answer is not readiness once the launcher's own child has
/// exited: when the child exits while the 200 answer is on its way, and when it exited before
/// the first probe, `await_health` fails. While the child lives, the same answer is readiness.
#[tokio::test]
async fn a_health_answer_after_the_child_exited_is_not_readiness() {
    use std::sync::atomic::AtomicUsize;
    // The child exits while the probe waits for the answer, which then arrives with 200.
    let engine = Arc::new(child());
    engine.ready.store(false, Ordering::Release);
    let requests = Arc::new(AtomicUsize::new(0));
    let (url, server) = health_server({
        let (engine, requests) = (engine.clone(), requests.clone());
        move || { requests.fetch_add(1, Ordering::SeqCst); engine.kill_for_test(); wait_for_exit(&engine); }
    }).await;
    let result = engine.await_health(&url, FIXTURE_KEY, Duration::from_secs(10)).await;
    assert_eq!(result.err().as_deref(), Some("managed engine exited before readiness"));
    assert_eq!(requests.load(Ordering::SeqCst), 1, "the 200 answer was not given, or the probe was repeated");
    assert!(!engine.is_alive());
    server.abort();

    // The child exited before the first probe: the endpoint is not even asked.
    let exited = child();
    exited.ready.store(false, Ordering::Release);
    exited.kill_for_test();
    wait_for_exit(&exited);
    let requests = Arc::new(AtomicUsize::new(0));
    let (url, server) = health_server({ let requests = requests.clone(); move || { requests.fetch_add(1, Ordering::SeqCst); } }).await;
    assert_eq!(exited.await_health(&url, FIXTURE_KEY, Duration::from_secs(10)).await.err().as_deref(), Some("managed engine exited before readiness"));
    assert_eq!(requests.load(Ordering::SeqCst), 0);

    // The control: the same answer while the child lives.
    let living = child();
    living.ready.store(false, Ordering::Release);
    assert_eq!(living.await_health(&url, FIXTURE_KEY, Duration::from_secs(10)).await, Ok(()));
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    living.stop().await;
    server.abort();

    // Nor is an answer that is not a success, from the living child: llama-server answers
    // `/health` with 503 while it loads the model, and only the 200 that follows is readiness
    // (adversarial review: accepting any status passed every case above).
    let requests = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route("/health", axum::routing::get({
        let requests = requests.clone();
        move || {
            let loading = requests.fetch_add(1, Ordering::SeqCst) < 2;
            async move {
                if loading { (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"error": {"code": 503, "message": "Loading model", "type": "unavailable_error"}}))) }
                else { (axum::http::StatusCode::OK, axum::Json(json!({"status": "ok"}))) }
            }
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let loading = child();
    loading.ready.store(false, Ordering::Release);
    assert_eq!(loading.await_health(&url, FIXTURE_KEY, Duration::from_secs(10)).await, Ok(()));
    assert_eq!(requests.load(Ordering::SeqCst), 3, "a 503 answer was taken as readiness");
    loading.stop().await;
    server.abort();
}

// managed-engine-runtime task 3.7: physical devices are observed independently, and headroom is
// enforced before startup and after loading (requirement 7).

const ABSENT: &str = "selected device is absent from pinned engine enumeration";
const ENGINE_HEADROOM: &str = "selected engine device lacks required memory headroom";
const CUDA_INVALID: &str = "invalid CUDA platform observation";
const CUDA_DISAGREE: &str = "CUDA engine and platform device observations disagree";
const CUDA_UNAVAILABLE: &str = "CUDA physical identity unavailable";
const ENGINE_AMBIGUOUS: &str = "selected device is listed more than once by pinned engine enumeration";
const CUDA_AMBIGUOUS: &str = "CUDA physical identity is ambiguous";
const APPLE_SILICON: &str = "unified Metal observation requires Apple silicon; discrete Metal memory is not qualified";
const UNIFIED_HEADROOM: &str = "Apple unified memory lacks required headroom";
const VULKAN_SHARED: &str = "Vulkan unified/system memory observation is unsupported; use native Metal or CPU";
const VULKAN_AMBIGUOUS: &str = "Vulkan physical identity is absent or ambiguous";
const PROFILE_DEVICES: &str = "observed physical devices differ from qualified serving inventory";
const CHANGED: &str = "runtime device identity changed during startup";
const OBSERVATION_FAILED: &str = "platform device observation failed or timed out";

/// What one observer run yields: recorded output, or output computed when the observer runs.
enum Reply { Output(Result<String, String>), Then(Box<dyn FnOnce() -> Result<String, String> + Send>) }

/// Device observers that replay recorded output in order and record each run as
/// `<program> <arguments>`, the engine's enumeration as `engine`.
struct Recorded { replies: Mutex<std::collections::VecDeque<Reply>>, runs: Mutex<Vec<String>>, engines: Mutex<Vec<PathBuf>> }

impl Recorded {
    fn new<S: Into<String>>(outputs: impl IntoIterator<Item = S>) -> Self {
        Self::replies(outputs.into_iter().map(|output| Reply::Output(Ok(output.into()))))
    }
    fn replies(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self { replies: Mutex::new(replies.into_iter().collect()), runs: Mutex::default(), engines: Mutex::default() }
    }
    fn runs(&self) -> Vec<String> { self.runs.lock().unwrap().clone() }
}

impl DeviceObservers for Recorded {
    fn output<'a>(&'a self, observer: Observer<'a>, args: &'a [&'a str]) -> impl std::future::Future<Output = Result<String, String>> + Send + 'a {
        let program = match observer {
            Observer::Engine(executable) => { self.engines.lock().unwrap().push(executable.to_owned()); "engine" }
            Observer::Tool(name) => name,
        };
        self.runs.lock().unwrap().push(std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" "));
        let reply = self.replies.lock().unwrap().pop_front().unwrap_or_else(|| panic!("no recorded output for {program} {args:?}"));
        std::future::ready(match reply { Reply::Output(output) => output, Reply::Then(compute) => compute() })
    }
}

const LIST_DEVICES: &str = "engine --list-devices";
const NVIDIA_SMI: &str = "nvidia-smi --query-gpu=pci.bus_id,uuid,name,memory.total,driver_version,memory.free --format=csv,noheader,nounits";
const ARM64: &str = "sysctl -n hw.optional.arm64";
const IOREG: &str = "ioreg -rd1 -c IOPlatformExpertDevice";
const MEMSIZE: &str = "sysctl -n hw.memsize";
const VM_STAT: &str = "vm_stat";
const SW_VERS: &str = "sw_vers -productVersion";
const VULKANINFO: &str = "vulkaninfo --summary";

/// `llama-server --list-devices` output (the pinned engine prints `  <selector>: <description>
/// (<total> MiB, <free> MiB free)` under a heading) listing `lines`.
fn listed(lines: &[&str]) -> String {
    std::iter::once("Available devices:".to_owned()).chain(lines.iter().map(|line| format!("  {line}"))).map(|line| line + "\n").collect()
}

/// One `nvidia-smi` row in `NVIDIA_QUERY` order.
fn smi_row(bus: &str, uuid: &str, name: &str, total: u64, driver: &str, free: u64) -> String {
    format!("{bus}, {uuid}, {name}, {total}, {driver}, {free}\n")
}

const GPU_A: &str = "GPU-00000000-aaaa-bbbb-cccc-dddddddddddd";
const GPU_B: &str = "GPU-00000001-aaaa-bbbb-cccc-dddddddddddd";

fn selectors(names: &[&str]) -> Vec<String> { names.iter().map(|name| name.to_string()).collect() }

fn device(selector: &str, id: &str, name: &str, memory_mib: u64, driver: &str) -> EngineDevice {
    EngineDevice { id: id.into(), name: name.into(), memory_mib, driver_version: driver.into(), engine_device: selector.into() }
}

/// The engine's enumeration gives each selected device's name, total and free memory: free
/// memory of at least the reserve (inclusive) and total memory above it, in selector order.
#[test]
fn the_engine_enumeration_gives_each_selected_device_with_inclusive_headroom() {
    let one = |line: &str, reserve: u64| engine_devices(&listed(&[line]), &selectors(&["CUDA0"]), reserve);
    let rtx = |total: u64| ("CUDA0".to_owned(), RTX_4060.to_owned(), total);
    // The recorded line with 255 MiB free is refused at a 256 MiB reserve; 256 MiB free is enough.
    assert_eq!(one("CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 255 MiB free)", 256), Err(ENGINE_HEADROOM.into()));
    assert_eq!(one("CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 256 MiB free)", 256), Ok(vec![rtx(8188)]));
    // Total memory must be above the reserve: equal is refused.
    assert_eq!(one("CUDA0: NVIDIA GeForce RTX 4060 (256 MiB, 256 MiB free)", 256), Err(ENGINE_HEADROOM.into()));
    assert_eq!(one("CUDA0: NVIDIA GeForce RTX 4060 (257 MiB, 256 MiB free)", 256), Ok(vec![rtx(257)]));
    // A selector absent from the enumeration, including one that is only a prefix of a listed one.
    assert_eq!(engine_devices(&listed(&["CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)"]), &selectors(&["CUDA1"]), 256), Err(ABSENT.into()));
    assert_eq!(engine_devices(&listed(&["CUDA10: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)"]), &selectors(&["CUDA1"]), 256), Err(ABSENT.into()));
    assert_eq!(engine_devices("", &selectors(&["CUDA0"]), 256), Err(ABSENT.into()));
    // A selector listed twice is ambiguous, whichever line comes first and even when both agree.
    for lines in [["CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)", "CUDA0: NVIDIA GeForce RTX 4070 (12282 MiB, 12000 MiB free)"],
        ["CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 200 MiB free)", "CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)"],
        ["CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)", "CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)"]] {
        assert_eq!(engine_devices(&listed(&lines), &selectors(&["CUDA0"]), 256), Err(ENGINE_AMBIGUOUS.into()), "{lines:?}");
    }
    // Several devices, in selector order whatever the listing order; a description may hold
    // parentheses; the devices that are not selected are not checked.
    let several = listed(&["Vulkan2: AMD Radeon 780M (512 MiB, 0 MiB free)", "Vulkan1: NVIDIA GeForce RTX 4060 Laptop GPU (rev 2) (8188 MiB, 7000 MiB free)",
        "Vulkan0: NVIDIA RTX A4000 (16376 MiB, 16000 MiB free)"]);
    assert_eq!(engine_devices(&several, &selectors(&["Vulkan0", "Vulkan1"]), 256), Ok(vec![
        ("Vulkan0".into(), "NVIDIA RTX A4000".into(), 16376), ("Vulkan1".into(), "NVIDIA GeForce RTX 4060 Laptop GPU (rev 2)".into(), 8188)]));
    assert_eq!(engine_devices(&several, &selectors(&["Vulkan1", "Vulkan2"]), 256), Err(ENGINE_HEADROOM.into()));
    assert_eq!(engine_devices(&listed(&["MTL0: Apple M4 Pro (18432 MiB, 18431 MiB free)"]), &selectors(&["MTL0"]), 4096), Ok(vec![("MTL0".into(), "Apple M4 Pro".into(), 18432)]));
    // Unparsable lines.
    for (line, error) in [("CUDA0: NVIDIA GeForce RTX 4060", "invalid engine device observation"), ("CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB)", "invalid engine device memory"),
        ("CUDA0: NVIDIA GeForce RTX 4060 (8 GiB, 7900 MiB free)", "invalid engine device memory"), ("CUDA0: NVIDIA GeForce RTX 4060 (-1 MiB, 7900 MiB free)", "invalid engine device capacity"),
        ("CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 GiB free)", "invalid engine device free memory")] {
        assert_eq!(one(line, 256), Err(error.into()), "{line}");
    }
}

/// `nvidia-smi`: the selector `CUDA<n>` is the `n`th GPU in PCI bus order, whose name equals the
/// engine's and whose free memory is at least the reserve; a row without six columns is refused.
#[test]
fn cuda_selectors_are_the_gpus_in_pci_bus_order_with_the_engines_names_and_free_memory() {
    let engine = |selectors: &[(&str, &str)]| -> Vec<ListedDevice> { selectors.iter().map(|(selector, name)| (selector.to_string(), name.to_string(), 8188)).collect() };
    let rtx = engine(&[("CUDA0", RTX_4060)]);
    let row = |name: &str, free: u64| smi_row("00000000:01:00.0", GPU_A, name, 8188, "550.54.14", free);
    assert_eq!(cuda_devices(&row(RTX_4060, 7900), rtx.clone(), 256), Ok(vec![device("CUDA0", GPU_A, RTX_4060, 8188, "550.54.14")]));
    // A name that differs from the engine's, and free memory below the reserve (inclusive).
    assert_eq!(cuda_devices(&row("NVIDIA GeForce RTX 4070", 7900), rtx.clone(), 256), Err(CUDA_DISAGREE.into()));
    assert_eq!(cuda_devices(&row(RTX_4060, 255), rtx.clone(), 256), Err(CUDA_DISAGREE.into()));
    assert_eq!(cuda_devices(&row(RTX_4060, 256), rtx.clone(), 256).map(|devices| devices.len()), Ok(1));
    // Rows without exactly six columns, anywhere in the output.
    for data in ["00000000:01:00.0, GPU-a, NVIDIA GeForce RTX 4060, 8188, 550.54.14\n".to_owned(),
        "00000000:01:00.0, GPU-a, NVIDIA GeForce RTX 4060, 8188, 550.54.14, 7900, 1\n".to_owned(),
        format!("{}\n{}", row(RTX_4060, 7900), "No devices were found")] {
        assert_eq!(cuda_devices(&data, rtx.clone(), 256), Err(CUDA_INVALID.into()), "{data}");
    }
    // Unparsable numbers, a selector beyond the listed GPUs, and no GPU at all.
    assert_eq!(cuda_devices(&smi_row("00000000:01:00.0", GPU_A, RTX_4060, 0, "550.54.14", 7900).replace(" 0,", " [N/A],"), rtx.clone(), 256), Err("invalid CUDA memory observation".into()));
    assert_eq!(cuda_devices(&smi_row("00000000:01:00.0", GPU_A, RTX_4060, 8188, "550.54.14", 0).replace(" 0\n", " [N/A]\n"), rtx.clone(), 256), Err("invalid CUDA free memory".into()));
    assert_eq!(cuda_devices(&row(RTX_4060, 7900), engine(&[("CUDA1", RTX_4060)]), 256), Err(CUDA_UNAVAILABLE.into()));
    assert_eq!(cuda_devices("", rtx, 256), Err(CUDA_UNAVAILABLE.into()));
    // Three identical GPUs listed out of PCI bus order: each selector maps by bus ID, so `CUDA1`
    // is the second-lowest and `CUDA2` the highest bus ID (hexadecimal, `09` < `0A` < `41`).
    let data = [smi_row("00000000:41:00.0", "GPU-41", RTX_4060, 8188, "550.54.14", 7000), smi_row("00000000:09:00.0", "GPU-09", RTX_4060, 8188, "550.54.14", 7100),
        smi_row("00000000:0A:00.0", "GPU-0A", RTX_4060, 8188, "550.54.14", 7200)].concat();
    let three = engine(&[("CUDA0", RTX_4060), ("CUDA1", RTX_4060), ("CUDA2", RTX_4060)]);
    assert_eq!(cuda_devices(&data, three, 256), Ok(vec![device("CUDA0", "GPU-09", RTX_4060, 8188, "550.54.14"),
        device("CUDA1", "GPU-0A", RTX_4060, 8188, "550.54.14"), device("CUDA2", "GPU-41", RTX_4060, 8188, "550.54.14")]));
    let data = [smi_row("00000000:41:00.0", GPU_B, "NVIDIA RTX A4000", 16376, "550.54.14", 16000), smi_row("00000000:01:00.0", GPU_A, RTX_4060, 8188, "550.54.14", 7900)].concat();
    assert_eq!(cuda_devices(&data, engine(&[("CUDA1", "NVIDIA RTX A4000")]), 256), Ok(vec![device("CUDA1", GPU_B, "NVIDIA RTX A4000", 16376, "550.54.14")]));
    assert_eq!(cuda_devices(&data, engine(&[("CUDA1", RTX_4060)]), 256), Err(CUDA_DISAGREE.into()));
    // Two rows with the same PCI bus ID leave the position of each GPU ambiguous.
    let data = [smi_row("00000000:01:00.0", GPU_A, RTX_4060, 8188, "550.54.14", 7900), smi_row("00000000:01:00.0", GPU_B, RTX_4060, 8188, "550.54.14", 7900)].concat();
    for selector in ["CUDA0", "CUDA1"] {
        assert_eq!(cuda_devices(&data, engine(&[(selector, RTX_4060)]), 256), Err(CUDA_AMBIGUOUS.into()), "{selector}");
    }
}

/// Recorded `ioreg -rd1 -c IOPlatformExpertDevice` output, shortened, with a synthetic identity.
const IOREG_OUTPUT: &str = "+-o J000AP  <class IOPlatformExpertDevice, id 0x100000000, registered, matched, active, busy 0 (1 ms), retain 30>\n    {\n      \"IOPolledInterface\" = \"AppleARMWatchdogTimerHibernateHandler is not serializable\"\n      \"manufacturer\" = <\"Apple Inc.\">\n      \"model\" = <\"Mac00,00\">\n      \"IOPlatformSerialNumber\" = \"SYNTHETIC00\"\n      \"IOPlatformUUID\" = \"00000000-1111-2222-3333-444444444444\"\n    }\n";

/// Recorded `vm_stat` output: 16 KiB pages, with `free`, `inactive` and `speculative` pages.
fn vm_stat(free: u64, inactive: u64, speculative: u64) -> String {
    format!("Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:                               {free:>9}.\nPages active:                              483823.\nPages inactive:                           {inactive:>9}.\nPages speculative:                        {speculative:>9}.\nPages throttled:                                0.\nPages wired down:                          236718.\n\"Translation faults\":                 21701332587.\nPages stored in compressor:                701041.\nSwapouts:                                45709918.\n")
}

/// Apple: Metal is observed only on Apple silicon, as one device whose memory is the machine's,
/// counted once, with available memory (free, inactive and speculative pages, capped at physical
/// memory) of at least the reserve.
#[test]
fn unified_memory_is_apple_silicon_with_available_memory_of_at_least_the_reserve() {
    assert_eq!(apple_silicon("1\n"), Ok(()));
    for intel in ["0\n", "", "1 1\n"] { assert_eq!(apple_silicon(intel), Err(APPLE_SILICON.into()), "{intel:?}"); }
    assert_eq!(apple_hardware_id(IOREG_OUTPUT), Ok("00000000-1111-2222-3333-444444444444"));
    assert_eq!(apple_hardware_id(&IOREG_OUTPUT.replace("IOPlatformUUID", "IOPlatformUUIDs")), Err("Apple physical hardware identity unavailable".into()));
    assert_eq!(apple_memory_mib("25769803776\n"), Ok(24576));
    assert_eq!(apple_memory_mib("24 GB\n"), Err("invalid Apple unified memory observation".into()));
    // Bytes become whole MiB rounded down, so that physical memory one byte short of the reserve
    // is not counted as reaching it (adversarial review: rounding up passed every case above).
    assert_eq!(apple_memory_mib("4294967295\n"), Ok(4095));
    assert_eq!(apple_memory_mib("4294967296\n"), Ok(4096));
    assert_eq!(apple_available_mib(&vm_stat(512_000, 0, 0), apple_memory_mib("4294967295\n").unwrap(), 4096), Err(UNIFIED_HEADROOM.into()));
    // 262,144 pages of 16 KiB are 4,096 MiB: one page fewer is refused at a 4,096 MiB reserve.
    assert_eq!(apple_available_mib(&vm_stat(100_000, 150_000, 12_143), 24576, 4096), Err(UNIFIED_HEADROOM.into()));
    assert_eq!(apple_available_mib(&vm_stat(100_000, 150_000, 12_144), 24576, 4096), Ok(4096));
    // Each of the three kinds counts.
    for (free, inactive, speculative) in [(262_144, 0, 0), (0, 262_144, 0), (0, 0, 262_144)] {
        assert_eq!(apple_available_mib(&vm_stat(free, inactive, speculative), 24576, 4096), Ok(4096));
    }
    // Capped at physical memory: 8,000 MiB available on a 4,095 MiB machine is not enough.
    assert_eq!(apple_available_mib(&vm_stat(512_000, 0, 0), 4095, 4096), Err(UNIFIED_HEADROOM.into()));
    assert_eq!(apple_available_mib(&vm_stat(512_000, 0, 0), 4096, 4096), Ok(4096));
    // The recorded output of a 24 GiB machine: 20,635 + 478,508 + 4,542 pages are 7,870 MiB.
    assert_eq!(apple_available_mib(&vm_stat(20_635, 478_508, 4_542), 24576, 4096), Ok(7870));
    // Unparsable output.
    let vm = vm_stat(1, 1, 1);
    assert_eq!(apple_available_mib(&vm.replacen("page size of 16384", "page size of many", 1), 24576, 0), Err("Apple VM page size unavailable".into()));
    assert_eq!(apple_available_mib(&vm.replace("Pages speculative", "Pages speculated"), 24576, 0), Err("Apple available memory observation unavailable".into()));
    assert_eq!(apple_available_mib(&vm_stat(u64::MAX, 1, 0), 24576, 0), Err("Apple memory observation overflow".into()));
    assert_eq!(apple_available_mib(&vm_stat(u64::MAX / 16384 + 1, 0, 0), 24576, 0), Err("Apple memory observation overflow".into()));
}

/// Recorded `vulkaninfo --summary` output, shortened: instance sections before the devices, a
/// discrete NVIDIA GPU, and a CPU (llvmpipe) device.
const VULKANINFO_OUTPUT: &str = "==========\nVULKANINFO\n==========\n\nVulkan Instance Version: 1.3.275\n\n\nInstance Extensions: count = 23\n-------------------------------\nVK_KHR_surface                         : extension revision 25\n\nInstance Layers: count = 3\n--------------------------\nVK_LAYER_MESA_device_select Linux device selection layer 1.3.211  version 1\n\nDevices:\n========\nGPU0:\n\tapiVersion         = 1.3.277\n\tdriverVersion      = 550.54.14.0\n\tvendorID           = 0x10de\n\tdeviceID           = 0x2882\n\tdeviceType         = PHYSICAL_DEVICE_TYPE_DISCRETE_GPU\n\tdeviceName         = NVIDIA GeForce RTX 4060\n\tdriverID           = DRIVER_ID_NVIDIA_PROPRIETARY\n\tdriverName         = NVIDIA\n\tdriverInfo         = 550.54.14\n\tconformanceVersion = 1.3.7.2\n\tdeviceUUID         = 5e8d6b1c-0000-1111-2222-333344445555\n\tdriverUUID         = 00000000-0000-0000-0000-000000000000\nGPU1:\n\tapiVersion         = 1.3.274\n\tdriverVersion      = 24.0.9\n\tvendorID           = 0x10005\n\tdeviceID           = 0x0000\n\tdeviceType         = PHYSICAL_DEVICE_TYPE_CPU\n\tdeviceName         = llvmpipe (LLVM 17.0.6, 256 bits)\n\tdriverID           = DRIVER_ID_MESA_LLVMPIPE\n\tdriverName         = llvmpipe\n\tdriverInfo         = Mesa 24.0.9-0ubuntu0.1 (LLVM 17.0.6)\n\tdeviceUUID         = 6d657361-3234-2e30-2e39-2d3075627500\n";

/// Vulkan: each engine device is the one discrete GPU of that name, with the engine's memory;
/// an integrated or CPU device, and two devices of the same name, are refused.
#[test]
fn vulkan_devices_are_the_one_discrete_gpu_of_that_name() {
    let engine = |name: &str| vec![("Vulkan0".to_owned(), name.to_owned(), 8188)];
    let rtx = device("Vulkan0", "5e8d6b1c-0000-1111-2222-333344445555", RTX_4060, 8188, "550.54.14");
    assert_eq!(vulkan_devices(VULKANINFO_OUTPUT, engine(RTX_4060)), Ok(vec![rtx.clone()]));
    // The integrated GPU of a shared-memory machine, and the CPU device.
    let integrated = VULKANINFO_OUTPUT.replace("PHYSICAL_DEVICE_TYPE_DISCRETE_GPU", "PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU");
    assert_eq!(vulkan_devices(&integrated, engine(RTX_4060)), Err(VULKAN_SHARED.into()));
    assert_eq!(vulkan_devices(&VULKANINFO_OUTPUT.replace("\tdeviceType         = PHYSICAL_DEVICE_TYPE_DISCRETE_GPU\n", ""), engine(RTX_4060)), Err(VULKAN_SHARED.into()));
    assert_eq!(vulkan_devices(VULKANINFO_OUTPUT, engine("llvmpipe (LLVM 17.0.6, 256 bits)")), Err(VULKAN_SHARED.into()));
    // Two devices with the same name, and a name that is not listed.
    let twins = VULKANINFO_OUTPUT.replace("llvmpipe (LLVM 17.0.6, 256 bits)", RTX_4060).replace("PHYSICAL_DEVICE_TYPE_CPU", "PHYSICAL_DEVICE_TYPE_DISCRETE_GPU");
    assert_eq!(vulkan_devices(&twins, engine(RTX_4060)), Err(VULKAN_AMBIGUOUS.into()));
    assert_eq!(vulkan_devices(VULKANINFO_OUTPUT, engine("NVIDIA GeForce RTX 4070")), Err(VULKAN_AMBIGUOUS.into()));
    // Identity: `driverVersion` when `driverInfo` is absent; no UUID is refused.
    let without_info = VULKANINFO_OUTPUT.replace("\tdriverInfo         = 550.54.14\n", "");
    assert_eq!(vulkan_devices(&without_info, engine(RTX_4060)), Ok(vec![EngineDevice { driver_version: "550.54.14.0".into(), ..rtx }]));
    assert_eq!(vulkan_devices(&VULKANINFO_OUTPUT.replace("\tdeviceUUID         = 5e8d6b1c-0000-1111-2222-333344445555\n", ""), engine(RTX_4060)), Err("Vulkan UUID unavailable".into()));
}

/// `observe_devices` runs the observers one after another, each only after the previous output
/// passed its checks: none for a CPU runtime, which observes memory kind `system`.
#[tokio::test]
async fn devices_are_observed_by_the_engine_and_platform_tools_in_order() {
    let executable = Path::new("/verified/runtime/llama-server");
    let config = |backend: &str, devices: &[&str], reserve: u64| {
        let (_directory, mut model) = fixture();
        if backend != "cpu" { accelerated(&mut model, backend, devices, reserve); }
        model.runtime.unwrap()
    };
    // CPU: no observer runs.
    let recorded = Recorded::new(Vec::<String>::new());
    assert_eq!(observe_devices(&config("cpu", &[], 0), executable, &recorded).await, Ok(("system".into(), vec![])));
    assert!(recorded.runs().is_empty());
    // CUDA: the engine's enumeration, then `nvidia-smi`.
    let cuda = config("cuda", &["CUDA0"], 256);
    let recorded = Recorded::new([listed(&["CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)"]), smi_row("00000000:01:00.0", GPU_A, RTX_4060, 8188, "550.54.14", 7900)]);
    assert_eq!(observe_devices(&cuda, executable, &recorded).await, Ok(("dedicated".into(), vec![device("CUDA0", GPU_A, RTX_4060, 8188, "550.54.14")])));
    assert_eq!(recorded.runs(), [LIST_DEVICES, NVIDIA_SMI]);
    assert_eq!(*recorded.engines.lock().unwrap(), [executable]);
    // Insufficient headroom on the engine's enumeration: `nvidia-smi` does not run.
    let recorded = Recorded::new([listed(&["CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 200 MiB free)"])]);
    assert_eq!(observe_devices(&cuda, executable, &recorded).await, Err(ENGINE_HEADROOM.into()));
    assert_eq!(recorded.runs(), [LIST_DEVICES]);
    // A failed observer fails the observation.
    let recorded = Recorded::replies([Reply::Output(Ok(listed(&["CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)"]))), Reply::Output(Err(OBSERVATION_FAILED.into()))]);
    assert_eq!(observe_devices(&cuda, executable, &recorded).await, Err(OBSERVATION_FAILED.into()));
    // Metal: the engine, then the Apple tools; the memory is the machine's, not the engine's.
    let mut metal = config("cuda", &["CUDA0"], 256);
    (metal.backend, metal.device_ids, metal.memory_reserve_mib) = ("metal".into(), selectors(&["MTL0"]), 4096);
    let apple = |arm64: &str, vm: String| Recorded::new([listed(&["MTL0: Apple M4 Pro (18432 MiB, 18431 MiB free)"]), arm64.to_owned(), IOREG_OUTPUT.to_owned(), "25769803776\n".to_owned(), vm, "27.0.1\n".to_owned()]);
    let recorded = apple("1\n", vm_stat(20_635, 478_508, 4_542));
    assert_eq!(observe_devices(&metal, executable, &recorded).await, Ok(("unified".into(), vec![device("MTL0", "00000000-1111-2222-3333-444444444444", "Apple M4 Pro", 24576, "27.0.1")])));
    assert_eq!(recorded.runs(), [LIST_DEVICES, ARM64, IOREG, MEMSIZE, VM_STAT, SW_VERS]);
    // Not Apple silicon: nothing after `sysctl`. Below the reserve: no `sw_vers`.
    let recorded = apple("0\n", vm_stat(20_635, 478_508, 4_542));
    assert_eq!(observe_devices(&metal, executable, &recorded).await, Err(APPLE_SILICON.into()));
    assert_eq!(recorded.runs(), [LIST_DEVICES, ARM64]);
    let recorded = apple("1\n", vm_stat(100_000, 150_000, 12_143));
    assert_eq!(observe_devices(&metal, executable, &recorded).await, Err(UNIFIED_HEADROOM.into()));
    assert_eq!(recorded.runs(), [LIST_DEVICES, ARM64, IOREG, MEMSIZE, VM_STAT]);
    // Vulkan: the engine, then `vulkaninfo --summary`; the memory is the engine's.
    let vulkan = config("vulkan", &["Vulkan0"], 256);
    let recorded = Recorded::new([listed(&["Vulkan0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)"]), VULKANINFO_OUTPUT.to_owned()]);
    assert_eq!(observe_devices(&vulkan, executable, &recorded).await, Ok(("dedicated".into(), vec![device("Vulkan0", "5e8d6b1c-0000-1111-2222-333344445555", RTX_4060, 8188, "550.54.14")])));
    assert_eq!(recorded.runs(), [LIST_DEVICES, VULKANINFO]);
    let recorded = Recorded::new([listed(&["Vulkan0: NVIDIA GeForce RTX 4060 (8188 MiB, 7900 MiB free)"]), VULKANINFO_OUTPUT.replace("PHYSICAL_DEVICE_TYPE_DISCRETE_GPU", "PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU")]);
    assert_eq!(observe_devices(&vulkan, executable, &recorded).await, Err(VULKAN_SHARED.into()));
}

/// The recorded CUDA observation of one RTX 4060 with `free` MiB free and driver `driver`.
fn rtx_4060(free: u64, driver: &str) -> [String; 2] {
    [listed(&[&format!("CUDA0: NVIDIA GeForce RTX 4060 (8188 MiB, {free} MiB free)")]), smi_row("00000000:01:00.0", GPU_A, RTX_4060, 8188, driver, free)]
}

/// `ManagedEngine::start` with recorded observations: devices that differ from the serving
/// profile, a driver update among them, or that lack headroom, refuse the launch before the port
/// check and the spawn; only the observation before startup ran.
#[tokio::test]
async fn observed_devices_that_differ_from_the_profile_or_lack_headroom_refuse_the_launch_before_spawning() {
    let (_directory, mut config) = cuda_fixture(&["CUDA0"]);
    let profile = profile_of(&config);
    assert_eq!(profile.devices, [device("CUDA0", GPU_A, RTX_4060, 8188, "550.54.14")]);
    config.model.serving = Some(profile.clone());
    config.engine_url = format!("http://127.0.0.1:{}", unused_port());
    // The signed driver: the launch passes the observation and fails only at the spawn of the
    // fixture's header-only executable.
    let recorded = Recorded::new(rtx_4060(7900, "550.54.14"));
    let error = ManagedEngine::start_observed(&config, &recorded).await.err().unwrap();
    assert!(failed_after_port_check(&error), "{error}");
    assert_eq!(recorded.runs(), [LIST_DEVICES, NVIDIA_SMI]);
    // A driver update, and with a listener on the endpoint: still the device error, not the
    // occupied port, so the launch stopped before the port check and the spawn.
    let listener = foreign_listener("127.0.0.1:0".parse().unwrap(), false).await;
    let occupied = { let mut config = config.clone(); config.engine_url = format!("http://{}", listener.local_addr().unwrap()); config };
    for config in [&config, &occupied] {
        let recorded = Recorded::new(rtx_4060(7900, "555.42.02"));
        assert_eq!(ManagedEngine::start_observed(config, &recorded).await.err().as_deref(), Some(PROFILE_DEVICES), "{}", config.engine_url);
        assert_eq!(recorded.runs(), [LIST_DEVICES, NVIDIA_SMI]);
    }
    // Every observed field of the signed device, and the memory kind.
    let changes: [(&str, fn(&mut crate::protocol::ServingProfile)); 5] = [
        ("physical identifier", |p| p.devices[0].id = GPU_B.into()), ("name", |p| p.devices[0].name = "NVIDIA GeForce RTX 4070".into()),
        ("physical memory", |p| p.devices[0].memory_mib = 8192), ("driver", |p| p.devices[0].driver_version = "555.42.02".into()),
        ("memory kind", |p| p.memory_kind = "unified".into())];
    for (name, change) in changes {
        let mut changed = config.clone();
        change(changed.model.serving.as_mut().unwrap());
        let recorded = Recorded::new(rtx_4060(7900, "550.54.14"));
        assert_eq!(ManagedEngine::start_observed(&changed, &recorded).await.err().as_deref(), Some(PROFILE_DEVICES), "{name}");
    }
    // Without a profile, the same driver update reaches the spawn.
    let mut unprofiled = config.clone();
    unprofiled.model.serving = None;
    let error = ManagedEngine::start_observed(&unprofiled, &Recorded::new(rtx_4060(7900, "555.42.02"))).await.err().unwrap();
    assert!(failed_after_port_check(&error), "{error}");
    // Insufficient headroom before startup: `CUDA0` reports 200 MiB free at a 256 MiB reserve.
    for config in [&unprofiled, &occupied] {
        let recorded = Recorded::new(rtx_4060(200, "550.54.14"));
        assert_eq!(ManagedEngine::start_observed(config, &recorded).await.err().as_deref(), Some(ENGINE_HEADROOM));
        assert_eq!(recorded.runs(), [LIST_DEVICES]);
    }
    assert_still_accepts(&listener, listener.local_addr().unwrap()).await;
}

/// After the engine reports health the devices are observed again. Lost headroom, a changed
/// device and an exited child each fail the launch and stop the child, which the helper child
/// stands in for; an unchanged observation with headroom makes the engine ready.
#[tokio::test]
async fn a_post_load_observation_that_loses_headroom_or_changes_stops_the_child() {
    let (_directory, mut config) = cuda_fixture(&["CUDA0"]);
    runtime_of(&mut config).startup_timeout_secs = 5;
    let (url, server) = health_server(|| {}).await;
    config.engine_url = url;
    let executable = Path::new("/verified/runtime/llama-server");
    // Before startup: a CUDA runtime reserving 256 MiB whose device had 4,000 MiB free.
    let before = observe_devices(config.runtime.as_ref().unwrap(), executable, &Recorded::new(rtx_4060(4000, "550.54.14"))).await.unwrap();
    let launched = || {
        let mut engine = child();
        engine.ready.store(false, Ordering::Release);
        (engine.observation.memory_kind, engine.observation.devices) = before.clone();
        Arc::new(engine)
    };
    let cases: Vec<(&str, [String; 2], &str)> = vec![
        ("100 MiB free on the engine's enumeration", [rtx_4060(100, "550.54.14")[0].clone(), rtx_4060(4000, "550.54.14")[1].clone()], ENGINE_HEADROOM),
        ("100 MiB free on nvidia-smi", [rtx_4060(4000, "550.54.14")[0].clone(), rtx_4060(100, "550.54.14")[1].clone()], CUDA_DISAGREE),
        ("a driver update", rtx_4060(4000, "555.42.02"), CHANGED),
        ("another physical device", [rtx_4060(4000, "550.54.14")[0].clone(), smi_row("00000000:01:00.0", GPU_B, RTX_4060, 8188, "550.54.14", 4000)], CHANGED),
    ];
    for (name, after, error) in cases {
        let engine = launched();
        let pid = engine.observation.pid;
        let recorded = Recorded::new(after);
        assert_eq!(engine.await_loaded(&config, executable, &recorded).await.err().as_deref(), Some(error), "{name}");
        assert_eq!(recorded.runs(), [LIST_DEVICES, NVIDIA_SMI][..recorded.runs().len()], "{name}");
        #[cfg(unix)]
        assert!(gone_within_five_seconds(pid).await, "{name}: the engine child was not stopped");
        #[cfg(not(unix))]
        let _ = pid;
    }
    // The child exits while the devices are observed again: the observation is unchanged, but
    // the launch fails.
    let engine = launched();
    let watched = engine.clone();
    let [list, smi] = rtx_4060(4000, "550.54.14");
    let recorded = Recorded::replies([Reply::Output(Ok(list)), Reply::Then(Box::new(move || { watched.kill_for_test(); wait_for_exit(&watched); Ok(smi) }))]);
    assert_eq!(engine.await_loaded(&config, executable, &recorded).await.err().as_deref(), Some(CHANGED), "the child exited");
    // The control: the same observation with 300 MiB free after loading is ready.
    let engine = launched();
    let recorded = Recorded::new(rtx_4060(300, "550.54.14"));
    let ready = engine.await_loaded(&config, executable, &recorded).await.unwrap();
    assert!(ready.is_alive());
    assert_eq!(recorded.runs(), [LIST_DEVICES, NVIDIA_SMI]);
    ready.stop().await;
    server.abort();
}

/// Writes what its mode argument asks for to standard output, for the bounds of `capture`.
#[test]
#[ignore = "device-observer helper, run only by device_observation_output_is_bounded_in_size_and_time"]
fn device_observer_helper() {
    use std::io::Write as _;
    if !probing() { return; }
    let mode = std::env::args().find_map(|arg| arg.strip_prefix("observer-mode=").map(str::to_owned)).unwrap_or_default();
    let mut stdout = std::io::stdout().lock();
    match mode.as_str() {
        "ok" => writeln!(stdout, "observer-ok").unwrap(),
        "under" => stdout.write_all(&vec![b'x'; 1024 * 1024 - 4096]).unwrap(),
        "oversize" => stdout.write_all(&vec![b'x'; 1024 * 1024 + 1]).unwrap(),
        "not-utf8" => stdout.write_all(b"\xff\xfe observer\n").unwrap(),
        "fails" => { writeln!(stdout, "observer-ok").unwrap(); stdout.flush().unwrap(); std::process::exit(3); }
        "hangs" => { writeln!(stdout, "observer-ok").unwrap(); stdout.flush().unwrap(); std::thread::sleep(Duration::from_secs(60)); }
        other => panic!("unknown observer mode {other:?}"),
    }
}

/// Observation output is bounded in size (1 MiB) and time (10 seconds), and must come from a
/// successful run as UTF-8; a missing observer or platform tool fails the observation.
#[tokio::test]
async fn device_observation_output_is_bounded_in_size_and_time() {
    let executable = std::env::current_exe().unwrap();
    let run = |mode: &'static str, timeout: Duration| {
        let executable = executable.clone();
        async move {
            let mut args = helper_args("engine_runtime::tests::device_observer_helper").to_vec();
            let mode = format!("observer-mode={mode}");
            args.push(&mode);
            capture_within(&executable, &args, timeout).await
        }
    };
    assert_eq!(OBSERVER_TIMEOUT, Duration::from_secs(10));
    assert!(run("ok", OBSERVER_TIMEOUT).await.unwrap().lines().any(|line| line == "observer-ok"));
    assert!(run("under", OBSERVER_TIMEOUT).await.unwrap().len() > 1024 * 1024 - 4096);
    for mode in ["oversize", "not-utf8", "fails"] {
        assert_eq!(run(mode, OBSERVER_TIMEOUT).await.err().as_deref(), Some(OBSERVATION_FAILED), "{mode}");
    }
    let started = std::time::Instant::now();
    assert_eq!(run("hangs", Duration::from_millis(500)).await.err().as_deref(), Some(OBSERVATION_FAILED), "hangs");
    assert!(started.elapsed() < Duration::from_secs(5), "{:?}", started.elapsed());
    // A missing observer, and a platform tool absent from the fixed system locations.
    assert_eq!(capture(&executable.with_file_name("chiral-no-such-observer"), &[]).await.err().as_deref(), Some("platform device observer is unavailable"));
    assert_eq!(system_tool("chiral-no-such-observer").err().as_deref(), Some("required platform observer chiral-no-such-observer is unavailable"));
    assert_eq!(SystemObservers.output(Observer::Tool("chiral-no-such-observer"), &[]).await.err().as_deref(), Some("required platform observer chiral-no-such-observer is unavailable"));
    #[cfg(unix)]
    assert!(["/usr/bin/sh", "/bin/sh"].contains(&system_tool("sh").unwrap().to_str().unwrap()));
}

/// The parsers accept this host's own Apple platform tools (macOS only; values not asserted).
#[cfg(target_os = "macos")]
#[tokio::test]
async fn this_hosts_apple_platform_tools_are_parsed() {
    let run = |tool: &'static str, args: &'static [&'static str]| SystemObservers.output(Observer::Tool(tool), args);
    let memory = apple_memory_mib(&run("sysctl", &["-n", "hw.memsize"]).await.unwrap()).unwrap();
    assert!(memory > 0);
    let hardware = run("ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"]).await.unwrap();
    assert_eq!(apple_hardware_id(&hardware).unwrap().len(), 36);
    let available = apple_available_mib(&run("vm_stat", &[]).await.unwrap(), memory, 0).unwrap();
    assert!(available <= memory);
    assert!(!run("sw_vers", &["-productVersion"]).await.unwrap().trim().is_empty());
    if cfg!(target_arch = "aarch64") { assert_eq!(apple_silicon(&run("sysctl", &["-n", "hw.optional.arm64"]).await.unwrap()), Ok(())); }
}

/// `ManagedEngine::start` remains a `Send` future after the observer seam (compile-time only).
#[allow(dead_code)]
fn start_is_send(model: &ModelConfig) {
    fn send<T: Send>(_: &T) {}
    send(&ManagedEngine::start(model));
}

// managed-engine-runtime task 3.9: startup is bounded, and a failed launch serves and advertises
// nothing (requirement 9).

/// Exits at once, as an engine that cannot load its model does.
#[test]
#[ignore = "exiting engine helper, run only by an_engine_that_exits_before_readiness_fails_the_launch_and_is_not_relaunched"]
fn exiting_child_helper() {}

/// Requirement 9's exited-before-readiness scenario: an engine child that exits at once, while
/// its endpoint keeps answering `/health` with 503 "Loading model" as llama-server does while it
/// loads, fails the launch with the exited-before-readiness error, not after the startup timeout.
/// Nothing starts it again: the engine keeps its one process identity, whose exit status is
/// collected, and is never alive. The daemon-level count of engine starts is
/// `daemon_engine_process_test`'s `an_engine_that_exits_before_readiness_fails_the_start_without_a_second_launch`.
#[tokio::test]
async fn an_engine_that_exits_before_readiness_fails_the_launch_and_is_not_relaunched() {
    use std::sync::atomic::AtomicUsize;
    let requests = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route("/health", axum::routing::get({
        let requests = requests.clone();
        move || {
            requests.fetch_add(1, Ordering::SeqCst);
            async { (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({"error": {"code": 503, "message": "Loading model", "type": "unavailable_error"}}))) }
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let engine = ManagedEngine::helper_running_for_test("test/helper", &url, &helper_args("engine_runtime::tests::exiting_child_helper"));
    engine.ready.store(false, Ordering::Release);
    let pid = engine.observation.pid;
    let result = engine.await_health(&url, FIXTURE_KEY, Duration::from_secs(20)).await;
    assert_eq!(result.err().as_deref(), Some("managed engine exited before readiness"));
    assert_eq!(engine.observation.pid, pid);
    assert!(engine.child.lock().unwrap().try_wait().unwrap().is_some(), "the engine child is still running");
    assert!(!engine.is_alive());
    server.abort();
}

/// The startup health probe presents the engine key, read from the configured key file as
/// `ManagedEngine::start` reads it, as its bearer: an endpoint that answers 200 only to that
/// bearer makes the engine ready, and every probe carried it. It follows no redirect: a 307 or
/// 308 to an endpoint that answers 200 is not readiness and that endpoint is never asked, so the
/// launch times out and stops the child.
#[tokio::test]
async fn the_health_probe_presents_the_engine_key_and_follows_no_redirect() {
    use std::sync::atomic::AtomicUsize;
    let seen = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().route("/health", axum::routing::get({
        let seen = seen.clone();
        move |headers: axum::http::HeaderMap| {
            let authorization = headers.get(axum::http::header::AUTHORIZATION).map(|value| value.to_str().unwrap().to_owned());
            seen.lock().unwrap().push(authorization.clone());
            async move {
                if authorization == Some(format!("Bearer {FIXTURE_KEY}")) { (axum::http::StatusCode::OK, axum::Json(json!({"status": "ok"}))) }
                else { (axum::http::StatusCode::UNAUTHORIZED, axum::Json(json!({"error": {"code": 401, "message": "Invalid API Key"}}))) }
            }
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let (_directory, mut config) = fixture();
    assert_eq!(read_secret(config.engine_api_key_file.as_ref().unwrap()).unwrap(), FIXTURE_KEY);
    config.engine_url = url;
    runtime_of(&mut config).startup_timeout_secs = 10;
    let engine = child();
    engine.ready.store(false, Ordering::Release);
    // A CPU runtime runs no device observer.
    let ready = Arc::new(engine).await_loaded(&config, Path::new("/verified/runtime/llama-server"), &Recorded::new(Vec::<String>::new())).await.unwrap();
    assert!(ready.is_alive());
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty());
    assert!(seen.iter().all(|authorization| authorization.as_deref() == Some("Bearer test-private-engine-token-32-characters")), "{seen:?}");
    ready.stop().await;
    server.abort();

    for status in [307u16, 308] {
        let target_requests = Arc::new(AtomicUsize::new(0));
        let (target, target_server) = health_server({ let target_requests = target_requests.clone(); move || { target_requests.fetch_add(1, Ordering::SeqCst); } }).await;
        let redirects = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = axum::Router::new().route("/health", axum::routing::get({
            let (redirects, location) = (redirects.clone(), format!("{target}/health"));
            move || {
                redirects.fetch_add(1, Ordering::SeqCst);
                let location = location.clone();
                async move { (axum::http::StatusCode::from_u16(status).unwrap(), [(axum::http::header::LOCATION, location)]) }
            }
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let engine = child();
        engine.ready.store(false, Ordering::Release);
        let result = engine.await_health(&url, FIXTURE_KEY, Duration::from_millis(1500)).await;
        assert_eq!(result.err().as_deref(), Some("managed engine startup timed out"), "{status}");
        assert!(redirects.load(Ordering::SeqCst) >= 1, "{status}: the probe never asked the engine's endpoint");
        assert_eq!(target_requests.load(Ordering::SeqCst), 0, "{status}: the probe followed the redirect");
        assert!(!engine.is_alive());
        assert!(engine.child.lock().unwrap().try_wait().unwrap().is_some(), "{status}: the timed-out child was not stopped");
        server.abort();
        target_server.abort();
    }
}

/// Runs the startup health probe of a helper engine against `health-url=<url>`, in the process
/// environment its parent test gives it, and prints the result.
#[tokio::test]
#[ignore = "health-probe child helper, run only by the_health_probe_uses_no_proxy"]
async fn health_probe_proxy_helper() {
    if !probing() { return; }
    let url = std::env::args().find_map(|arg| arg.strip_prefix("health-url=").map(str::to_owned)).unwrap();
    let engine = child();
    engine.ready.store(false, Ordering::Release);
    let result = engine.await_health(&url, FIXTURE_KEY, Duration::from_secs(5)).await;
    println!("probe-result\t{result:?}");
    engine.stop().await;
}

/// The startup health probe uses no proxy: in a process whose environment names an HTTP proxy
/// for every scheme and no exception, the probe reaches the engine's endpoint directly and the
/// proxy receives no connection. The proxy accepts and closes each connection, so a proxied
/// probe would not be ready.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_health_probe_uses_no_proxy() {
    use std::sync::atomic::AtomicUsize;
    let proxied = Arc::new(AtomicUsize::new(0));
    let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let proxy_server = tokio::spawn({
        let proxied = proxied.clone();
        async move { while let Ok((connection, _)) = proxy.accept().await { proxied.fetch_add(1, Ordering::SeqCst); drop(connection); } }
    });
    let requests = Arc::new(AtomicUsize::new(0));
    let (url, server) = health_server({ let requests = requests.clone(); move || { requests.fetch_add(1, Ordering::SeqCst); } }).await;
    let health = format!("health-url={url}");
    let mut probe = tokio::process::Command::new(std::env::current_exe().unwrap());
    probe.args(helper_args("engine_runtime::tests::health_probe_proxy_helper")).arg(&health)
        .env_remove("NO_PROXY").env_remove("no_proxy").kill_on_drop(true);
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] { probe.env(name, &proxy_url); }
    let output = tokio::time::timeout(Duration::from_secs(60), probe.output()).await.expect("the probe helper did not finish").unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.lines().any(|line| line == "probe-result\tOk(())"), "{stdout}");
    assert!(requests.load(Ordering::SeqCst) >= 1);
    assert_eq!(proxied.load(Ordering::SeqCst), 0, "the probe connected to the proxy");
    server.abort();
    proxy_server.abort();
}

// managed-engine-runtime task 3.10: an engine exit withdraws service, and nothing restarts the
// engine (requirement 10). The provider-level test is in `app::tests`; this is its engine.

/// Appends this process's identifier and a newline to the file named by `spawn-log=<path>`,
/// then sleeps: an engine process whose starts a test counts.
#[test]
#[ignore = "recording engine helper, run only through ManagedEngine::recording_helper_for_test"]
fn recording_child_helper() {
    if !probing() { return; }
    let log = std::env::args().find_map(|arg| arg.strip_prefix("spawn-log=").map(PathBuf::from)).unwrap();
    std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap().write_all(format!("{}\n", std::process::id()).as_bytes()).unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

// managed-engine-runtime task 3.11: shutdown and parent termination stop the engines the daemon
// owns (requirement 11).

/// Kills, when dropped before `disarm`, the processes a failed process test left behind.
#[cfg(unix)]
struct Leftovers(Vec<u32>);

#[cfg(unix)]
impl Leftovers {
    /// Called once the processes are known to be gone, so that a reused identifier is never killed.
    fn disarm(mut self) { self.0.clear(); }
}

#[cfg(unix)]
impl Drop for Leftovers {
    fn drop(&mut self) {
        for pid in &self.0 { unsafe { libc::kill(*pid as i32, libc::SIGKILL); } }
    }
}

/// Whether `pid` exists, a zombie included.
#[cfg(unix)]
fn exists(pid: u32) -> bool { unsafe { libc::kill(pid as i32, 0) == 0 } }

/// Starts a grandchild, `managed_child_helper`, in this helper's process group (a spawned child's
/// default), writes its process identifier to the file named by `pid-file=<path>`, then sleeps.
#[cfg(unix)]
#[test]
#[ignore = "process-group child helper, run only by stopping_or_dropping_an_engine_kills_its_whole_process_group"]
fn process_group_child_helper() {
    if !probing() { return; }
    let path = std::env::args().find_map(|arg| arg.strip_prefix("pid-file=").map(PathBuf::from)).unwrap();
    let grandchild = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "engine_runtime::tests::managed_child_helper", "--ignored"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, grandchild.id().to_string()).unwrap();
    std::fs::rename(temporary, path).unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

/// Stopping an engine, and dropping its handle, kill its whole process group: the engine child
/// leads its own group, and a grandchild it started in that group is gone within five seconds
/// along with it.
#[cfg(unix)]
#[tokio::test]
async fn stopping_or_dropping_an_engine_kills_its_whole_process_group() {
    for how in ["stop", "drop", "bridge-exit"] {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("grandchild.pid");
        let argument = format!("pid-file={}", pid_file.display());
        let args: Vec<&str> = helper_args("engine_runtime::tests::process_group_child_helper").into_iter().chain([argument.as_str()]).collect();
        let engine = ManagedEngine::helper_running_for_test("test/helper", "http://127.0.0.1:1", &args);
        let engine_pid = engine.observation.pid;
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let grandchild = loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file).ok().and_then(|pid| pid.trim().parse::<u32>().ok()) { break pid; }
            assert!(std::time::Instant::now() < deadline, "{how}: the helper started no grandchild");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        let leftovers = Leftovers(vec![grandchild]);
        assert!(exists(grandchild), "{how}");
        assert_eq!(unsafe { libc::getpgid(engine_pid as i32) }, engine_pid as i32, "{how}: the engine does not lead its own process group");
        assert_eq!(unsafe { libc::getpgid(grandchild as i32) }, engine_pid as i32, "{how}: the grandchild is not in the engine's process group");
        if how == "stop" {
            engine.stop().await;
            assert!(engine.child.lock().unwrap().try_wait().unwrap().is_some(), "{how}: the engine child is still running");
            assert!(gone_within_five_seconds(grandchild).await, "{how}: the grandchild outlived the engine");
            drop(engine);
        } else if how == "bridge-exit" {
            engine.kill_for_test();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while engine.is_alive() && std::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert!(!engine.is_alive());
            assert!(gone_within_five_seconds(grandchild).await, "a crashed bridge left its compute descendant alive");
            drop(engine);
        } else {
            drop(engine);
            assert!(gone_within_five_seconds(engine_pid).await, "{how}: the engine child outlived its handle");
            assert!(gone_within_five_seconds(grandchild).await, "{how}: the grandchild outlived the engine");
        }
        leftovers.disarm();
    }
}

/// Listens on `127.0.0.1:<port>` (`port=<port>`) as a loaded engine does: answers each request
/// with 200 and appends its request line to the file named by `request-log=<path>`. It ends
/// itself after 60 seconds, so that an orphan a failed run leaves behind does not stay long.
#[test]
#[ignore = "listening engine helper, run only by a_killed_parent_takes_its_engine_with_it_on_linux_and_a_macos_orphan_blocks_the_next_start"]
fn listening_child_helper() {
    use std::io::{BufRead, Write as _};
    if !probing() { return; }
    let argument = |name: &str| std::env::args().find_map(|arg| arg.strip_prefix(name).map(str::to_owned)).unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", argument("port=").parse::<u16>().unwrap())).unwrap();
    let log = PathBuf::from(argument("request-log="));
    std::thread::spawn(|| { std::thread::sleep(Duration::from_secs(60)); std::process::exit(0); });
    for mut stream in listener.incoming().flatten() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut request = String::new();
        if std::io::BufReader::new(&stream).read_line(&mut request).is_ok_and(|read| read > 0) {
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&log) { let _ = file.write_all(request.as_bytes()); }
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"status\":\"ok\"}");
        }
    }
}

/// Holds a helper engine, `listening_child_helper` with this helper's `port=` and `request-log=`
/// arguments, started through the launcher's `command` (with, on Linux, its parent-death
/// signal); prints its process identifier, then sleeps until it is killed.
#[tokio::test]
#[ignore = "parent-death parent helper, run only by a_killed_parent_takes_its_engine_with_it_on_linux_and_a_macos_orphan_blocks_the_next_start"]
async fn parent_death_parent_helper() {
    if !probing() { return; }
    let passed: Vec<String> = std::env::args().filter(|arg| arg.starts_with("port=") || arg.starts_with("request-log=")).collect();
    let args: Vec<&str> = helper_args("engine_runtime::tests::listening_child_helper").into_iter().chain(passed.iter().map(String::as_str)).collect();
    let engine = ManagedEngine::helper_running_for_test("test/helper", "http://127.0.0.1:1", &args);
    println!("engine-pid\t{}", engine.observation.pid);
    std::io::stdout().flush().unwrap();
    std::thread::sleep(Duration::from_secs(60));
    drop(engine);
}

/// Requirement 11's abrupt end of the daemon. A parent helper that started a listening engine
/// child through the launcher's `command` is killed with `SIGKILL`, so no `Drop` or `stop` runs.
/// - On Linux the parent-death signal ends the child within five seconds, and a launch on the
///   same endpoint then passes the occupied-port check.
/// - macOS has no parent-death signal (*A macOS orphan blocks the next start instead of being
///   adopted*): the child outlives its parent and keeps listening, and a launch with the same
///   endpoint fails with the occupied-port error. The orphan is neither used (it logged no
///   request) nor signalled (it still runs and accepts a connection); the test then removes it.
#[cfg(unix)]
#[tokio::test]
async fn a_killed_parent_takes_its_engine_with_it_on_linux_and_a_macos_orphan_blocks_the_next_start() {
    use std::io::BufRead;
    /// Kills and reaps the parent helper, also after a failed assertion.
    struct Parent(std::process::Child);
    impl Drop for Parent {
        fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); }
    }
    let directory = tempfile::tempdir().unwrap();
    let request_log = directory.path().join("requests.log");
    let port = unused_port();
    let endpoint = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut parent = Parent(std::process::Command::new(std::env::current_exe().unwrap())
        .args(helper_args("engine_runtime::tests::parent_death_parent_helper"))
        .arg(format!("port={port}")).arg(format!("request-log={}", request_log.display()))
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap());
    let mut lines = std::io::BufReader::new(parent.0.stdout.take().unwrap()).lines();
    let engine = lines.by_ref().map_while(Result::ok).find_map(|line| line.strip_prefix("engine-pid\t").and_then(|pid| pid.parse::<u32>().ok()))
        .expect("the parent helper printed no engine process identifier");
    let leftovers = Leftovers(vec![engine]);
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !connects(endpoint).await {
        assert!(std::time::Instant::now() < deadline, "the engine child never listened");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(exists(engine), "the engine child is not running");
    // Abrupt: `SIGKILL`, so the parent runs no `Drop` or `stop`.
    drop(parent);
    let (_fixture, mut config) = fixture();
    config.engine_url = format!("http://{endpoint}");
    if cfg!(target_os = "linux") {
        assert!(gone_within_five_seconds(engine).await, "the engine child outlived its killed parent");
        let error = ManagedEngine::start(&config).await.err().unwrap();
        assert!(failed_after_port_check(&error), "{error}");
    } else {
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(exists(engine), "the engine child ended with its parent although this platform has no parent-death signal");
        assert_eq!(ManagedEngine::start(&config).await.err().as_deref(), Some(OCCUPIED_ENGINE_PORT));
        assert!(exists(engine), "the launch signalled the orphan");
        assert!(connects(endpoint).await, "the orphan stopped listening");
        assert_eq!(std::fs::read_to_string(&request_log).unwrap_or_default(), "", "the launch used the orphan");
        unsafe { libc::kill(engine as i32, libc::SIGKILL); }
        assert!(gone_within_five_seconds(engine).await, "the orphan was not removed");
    }
    leftovers.disarm();
}
